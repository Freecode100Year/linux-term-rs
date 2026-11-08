#!/usr/bin/env bash
set -euo pipefail

echo "===> 1. Ensuring musl target is installed..."
rustup target add x86_64-unknown-linux-musl

echo "===> 2. Compiling optimized release binary with musl..."
cargo build --release --target x86_64-unknown-linux-musl

TARGET_BIN="target/x86_64-unknown-linux-musl/release/linux-term-rs"

echo "===> 3. Verifying static binary properties..."
file "$TARGET_BIN"
if ldd "$TARGET_BIN" 2>&1 | grep -E -q "not a dynamic executable|statically linked"; then
    echo "✔ Binary verified: 100% statically linked."
else
    echo "❌ Verification failed: dynamic dependencies detected!"
    exit 1
fi

echo "===> 4. Creating release tarball..."
mkdir -p dist/linux-term-rs-x86_64
cp "$TARGET_BIN" dist/linux-term-rs-x86_64/
cp install.sh dist/linux-term-rs-x86_64/ 2>/dev/null || true
tar -czvf linux-term-rs-v0.1.0-x86_64-unknown-linux-musl.tar.gz -C dist/linux-term-rs-x86_64 .
sha256sum linux-term-rs-v0.1.0-x86_64-unknown-linux-musl.tar.gz > linux-term-rs-v0.1.0-x86_64-unknown-linux-musl.tar.gz.sha256

echo "✔ Build and packaging complete."
