# 🚀 linux-term-rs

A production-grade, zero-dependency, statically linked POSIX terminal emulator and shell written in Rust.

[![Build & Release](https://github.com/Freecode100Year/linux-term-rs/actions/workflows/release.yml/badge.svg)](https://github.com/Freecode100Year/linux-term-rs/actions)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

---

## ⚡ Quick Install (Any Linux Distro)

One-liner auto-detects system architecture (`x86_64` or `aarch64`) and installs the musl-static standalone binary:

```bash
curl -sSL https://raw.githubusercontent.com/Freecode100Year/linux-term-rs/main/install.sh | bash
```

---

## 🌟 Key Features

- **100% Musl Statically Linked**: Single binary, zero dynamic dependencies. Runs seamlessly on Debian, Ubuntu, CentOS, Fedora, Alpine, or raw BusyBox containers without `glibc` mismatch errors.
- **Full PTY Session Subsystem**: Powered by `libc::forkpty` with terminal raw mode switching (`termios`) and dynamic window size synchronization (`SIGWINCH`/`ioctl`). Runs interactive full-screen curses apps (`vim`, `htop`, `ssh`, `less`) flawlessly.
- **Pipelines & Redirections**: Supports arbitrary multi-stage UNIX pipe chains (`cmd1 | cmd2 | cmd3`) and file redirections (`<`, `>`, `>>`).
- **Dynamic Variable Expansion**: Inlines `$VAR` or `${VAR}` environment values at runtime.
- **Interactive Editing & History**: Full arrow key navigation, `Ctrl+R` reverse search, and auto-history persistence to `~/.linux_term_rs_history`.
- **Ultra-compact Size**: Optimized with `opt-level = "z"`, Link Time Optimization (`lto = true`), and stripped symbols.

---

## 🛠️ Build from Source

```bash
# Clone
git clone https://github.com/Freecode100Year/linux-term-rs.git
cd linux-term-rs

# Compile static binary
chmod +x build.sh
./build.sh
```
