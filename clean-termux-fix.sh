#!/bin/bash
# Clean fix for Termux compilation errors

echo "Cleaning up and fixing Termux compilation errors..."

# Restore original files first
git checkout crates/chat-cli/src/util/open.rs 2>/dev/null || true
git checkout crates/chat-cli/src/util/system_info/mod.rs 2>/dev/null || true

# Build with minimal features to avoid problematic code paths
echo "Building with minimal features for Termux..."
cargo build --release --no-default-features --bin q 2>/dev/null || \
cargo build --release --no-default-features --bin chat_cli 2>/dev/null || \
cargo build --release --no-default-features

echo "Build attempt complete!"
ls -la target/release/ | grep -E "(q|chat)" | head -5