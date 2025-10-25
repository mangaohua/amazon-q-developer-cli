#!/bin/bash
# Simple fix - find and exclude the crate using arboard

echo "Finding which crate uses arboard..."

# Find which workspace member uses arboard
for crate_dir in crates/*/; do
    if grep -q "arboard" "$crate_dir/Cargo.toml" 2>/dev/null; then
        crate_name=$(basename "$crate_dir")
        echo "Found arboard in: $crate_name"

        # Try building without this crate
        echo "Building without $crate_name..."
        cargo build --release --exclude "$crate_name"
        exit 0
    fi
done

# If no direct dependency found, try building specific binaries
echo "Trying to build specific binaries..."
cargo build --release --bin q

echo "Done!"