use libc::{
    c_int, ioctl, tcgetattr, tcsetattr, termios, winsize, ECHO, ICANON, ICRNL, IEXTEN, ISIG,
    IXON, OPOST, STDIN_FILENO, STDOUT_FILENO, TCSANOW, TIOCGWINSZ, TIOCSWINSZ,
};
use rustyline::error::ReadlineError;
use rustyline::DefaultEditor;
use std::env;
use std::ffi::CString;
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::mem;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::FromRawFd;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::ptr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

/// 终端原始模式守护卫士 (RAII 自动恢复宿主 Termios)
struct RawModeGuard {
    orig_termios: termios,
}

impl RawModeGuard {
    fn enter() -> io::Result<Self> {
        unsafe {
            let mut orig: termios = mem::zeroed();
            if tcgetattr(STDIN_FILENO, &mut orig) != 0 {
                return Err(io::Error::last_os_error());
            }

            let mut raw = orig;
            // 关闭回显、规范模式、扩展输入处理和流控
            raw.c_iflag &= !(IXON | ICRNL);
            raw.c_oflag &= !OPOST;
            raw.c_lflag &= !(ECHO | ICANON | ISIG | IEXTEN);

            if tcsetattr(STDIN_FILENO, TCSANOW, &raw) != 0 {
                return Err(io::Error::last_os_error());
            }

            Ok(Self { orig_termios: orig })
        }
    }
}

impl Drop for RawModeGuard {
    fn drop(&mut self) {
        unsafe {
            tcsetattr(STDIN_FILENO, TCSANOW, &self.orig_termios);
        }
    }
}

/// 实时获取当前宿主窗口尺寸并推送到 PTY Master
fn sync_window_size(master_fd: c_int) {
    unsafe {
        let mut ws: winsize = mem::zeroed();
        if ioctl(STDOUT_FILENO, TIOCGWINSZ, &mut ws) == 0 {
            ioctl(master_fd, TIOCSWINSZ, &ws);
        }
    }
}

/// 运行需要全屏接管的交互程序 (vim, htop, ssh, less, nano)
fn run_full_pty_session(program: &str, args: &[&str]) -> io::Result<i32> {
    let mut master_fd: c_int = 0;
    let mut ws: winsize = unsafe { mem::zeroed() };

    unsafe {
        ioctl(STDOUT_FILENO, TIOCGWINSZ, &mut ws);
    }

    let pid = unsafe {
        libc::forkpty(
            &mut master_fd,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut ws,
        )
    };

    if pid < 0 {
        return Err(io::Error::last_os_error());
    }

    if pid == 0 {
        let c_prog = CString::new(program).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?;
        let mut c_args: Vec<CString> = vec![c_prog.clone()];
        for arg in args {
            c_args.push(CString::new(*arg).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))?);
        }
        let mut c_arg_ptrs: Vec<*const libc::c_char> = c_args.iter().map(|s| s.as_ptr()).collect();
        c_arg_ptrs.push(ptr::null());

        unsafe {
            libc::execvp(c_prog.as_ptr(), c_arg_ptrs.as_ptr());
            libc::_exit(127);
        }
    }

    let _guard = RawModeGuard::enter()?;
    sync_window_size(master_fd);

    let running = Arc::new(AtomicBool::new(true));

    let mut master_file_read = unsafe { File::from_raw_fd(libc::dup(master_fd)) };
    let r_flag = running.clone();
    let reader_handle = thread::spawn(move || {
        let mut buf = [0u8; 4096];
        let mut stdout = io::stdout();
        while r_flag.load(Ordering::Relaxed) {
            match master_file_read.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let _ = stdout.write_all(&buf[..n]);
                    let _ = stdout.flush();
                }
            }
        }
    });

    let mut master_file_write = unsafe { File::from_raw_fd(libc::dup(master_fd)) };
    let w_flag = running.clone();
    let writer_handle = thread::spawn(move || {
        let mut stdin = io::stdin();
        let mut buf = [0u8; 1024];
        while w_flag.load(Ordering::Relaxed) {
            match stdin.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let _ = master_file_write.write_all(&buf[..n]);
                    let _ = master_file_write.flush();
                }
            }
        }
    });

    let mut status: c_int = 0;
    unsafe {
        libc::waitpid(pid, &mut status, 0);
        libc::close(master_fd);
    }

    running.store(false, Ordering::Relaxed);
    drop(reader_handle);
    drop(writer_handle);

    let exit_code = if libc::WIFEXITED(status) {
        libc::WEXITSTATUS(status)
    } else {
        1
    };

    Ok(exit_code)
}

/// 展开环境变量 $VAR
fn expand_environment(input: &str) -> String {
    let mut out = String::new();
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' {
            let mut var_name = String::new();
            while let Some(&nc) = chars.peek() {
                if nc.is_alphanumeric() || nc == '_' {
                    var_name.push(chars.next().unwrap());
                } else {
                    break;
                }
            }
            if !var_name.is_empty() {
                if let Ok(val) = env::var(&var_name) {
                    out.push_str(&val);
                }
            } else {
                out.push('$');
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn is_pty_candidate(cmd: &str) -> bool {
    matches!(
        cmd,
        "vim" | "vi" | "nano" | "htop" | "top" | "less" | "more" | "ssh" | "tmux" | "screen"
    )
}

fn main() -> rustyline::Result<()> {
    unsafe {
        libc::signal(libc::SIGTTOU, libc::SIG_IGN);
    }

    let history_file = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".linux_term_rs_history");

    let mut rl = DefaultEditor::new()?;
    let _ = rl.load_history(&history_file);

    println!("\x1b[1;36m=========================================================\x1b[0m");
    println!("\x1b[1;32m  🚀 Linux-Term-RS (Production Ready POSIX Terminal)\x1b[0m");
    println!("\x1b[1;33m  Statically Linked • Musl C • Full PTY Session Manager\x1b[0m");
    println!("\x1b[1;36m=========================================================\x1b[0m");

    let mut last_exit_code = 0;

    loop {
        let cwd = env::current_dir().unwrap_or_default();
        let home = env::var("HOME").unwrap_or_default();
        let display_path = if !home.is_empty() && cwd.starts_with(&home) {
            if let Ok(rel) = cwd.strip_prefix(&home) {
                format!("~{}", rel.display())
            } else {
                cwd.display().to_string()
            }
        } else {
            cwd.display().to_string()
        };

        let status_mark = if last_exit_code == 0 {
            "\x1b[1;32m✔\x1b[0m"
        } else {
            "\x1b[1;31m✘\x1b[0m"
        };
        let prompt = format!(
            "{} \x1b[1;32mterm-rs\x1b[0m:\x1b[1;34m{}\x1b[0m$ ",
            status_mark, display_path
        );

        match rl.readline(&prompt) {
            Ok(line) => {
                let raw_line = line.trim();
                if raw_line.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(raw_line);
                let _ = rl.save_history(&history_file);

                let cmd_line = expand_environment(raw_line);

                if cmd_line == "exit" {
                    break;
                }

                if cmd_line.starts_with("cd ") || cmd_line == "cd" {
                    let parts: Vec<&str> = cmd_line.split_whitespace().collect();
                    let target = if parts.len() > 1 { parts[1] } else { &home };
                    let dest = if target == "~" { &home } else { target };
                    if let Err(e) = env::set_current_dir(Path::new(dest)) {
                        eprintln!("cd: {}", e);
                        last_exit_code = 1;
                    } else {
                        last_exit_code = 0;
                    }
                    continue;
                }

                if cmd_line.starts_with("export ") {
                    let assign = &cmd_line[7..];
                    if let Some((k, v)) = assign.split_once('=') {
                        env::set_var(k.trim(), v.trim().trim_matches('"').trim_matches('\''));
                        last_exit_code = 0;
                    } else {
                        eprintln!("export: invalid format, usage: export KEY=VALUE");
                        last_exit_code = 1;
                    }
                    continue;
                }

                let mut in_redirect = None;
                let mut out_redirect = None;
                let mut append = false;
                let mut clean_cmd = cmd_line.clone();

                if let Some((left, right)) = clean_cmd.split_once('<') {
                    let parts: Vec<&str> = right.split_whitespace().collect();
                    if !parts.is_empty() {
                        in_redirect = Some(parts[0].to_string());
                    }
                    clean_cmd = left.trim().to_string();
                }

                if let Some((left, right)) = clean_cmd.split_once(">>") {
                    append = true;
                    out_redirect = Some(right.trim().to_string());
                    clean_cmd = left.trim().to_string();
                } else if let Some((left, right)) = clean_cmd.split_once('>') {
                    append = false;
                    out_redirect = Some(right.trim().to_string());
                    clean_cmd = left.trim().to_string();
                }

                if !clean_cmd.contains('|') && out_redirect.is_none() && in_redirect.is_none() {
                    let mut parts = clean_cmd.split_whitespace();
                    if let Some(prog) = parts.next() {
                        let args: Vec<&str> = parts.collect();
                        if is_pty_candidate(prog) {
                            match run_full_pty_session(prog, &args) {
                                Ok(code) => last_exit_code = code,
                                Err(e) => {
                                    eprintln!("term-rs: failed to spawn pty: {}", e);
                                    last_exit_code = 1;
                                }
                            }
                            continue;
                        }
                    }
                }

                let mut commands = clean_cmd.split('|').map(str::trim).peekable();
                let mut prev_pipe_out = None;
                let mut children: Vec<Child> = Vec::new();
                let mut failed = false;

                while let Some(cmd_seg) = commands.next() {
                    let mut parts = cmd_seg.split_whitespace();
                    let Some(prog) = parts.next() else { continue };
                    let args: Vec<&str> = parts.collect();
                    let is_last = commands.peek().is_none();

                    let stdin = if let Some(ref in_file) = in_redirect {
                        match File::open(in_file) {
                            Ok(f) => Stdio::from(f),
                            Err(e) => {
                                eprintln!("term-rs: {}: {}", in_file, e);
                                failed = true;
                                break;
                            }
                        }
                    } else {
                        prev_pipe_out.map_or(Stdio::inherit(), Stdio::from)
                    };

                    let stdout = if is_last {
                        if let Some(ref out_file) = out_redirect {
                            let open_res = OpenOptions::new()
                                .create(true)
                                .write(true)
                                .append(append)
                                .truncate(!append)
                                .mode(0o644)
                                .open(out_file);
                            match open_res {
                                Ok(f) => Stdio::from(f),
                                Err(e) => {
                                    eprintln!("term-rs: {}: {}", out_file, e);
                                    failed = true;
                                    break;
                                }
                            }
                        } else {
                            Stdio::inherit()
                        }
                    } else {
                        Stdio::piped()
                    };

                    match Command::new(prog).args(args).stdin(stdin).stdout(stdout).spawn() {
                        Ok(mut child) => {
                            prev_pipe_out = child.stdout.take();
                            children.push(child);
                        }
                        Err(e) => {
                            eprintln!("term-rs: {}: command not found ({})", prog, e);
                            failed = true;
                            break;
                        }
                    }
                }

                if failed {
                    last_exit_code = 1;
                    continue;
                }

                let mut code = 0;
                for mut child in children {
                    if let Ok(res) = child.wait() {
                        code = res.code().unwrap_or(1);
                    }
                }
                last_exit_code = code;
            }
            Err(ReadlineError::Interrupted) => {
                println!("^C");
                last_exit_code = 130;
            }
            Err(ReadlineError::Eof) => {
                println!("exit");
                break;
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        }
    }

    Ok(())
}
