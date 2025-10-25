#!/bin/bash
# Clean up unnecessary files and create single commit

echo "Cleaning up unnecessary fix files..."

# Remove all the fix scripts we created
rm -f android-build.sh
rm -f android-build-simple.sh
rm -f build-termux.sh
rm -f termux-direct-build.sh
rm -f fix-termux-build.sh
rm -f fix-termux-clipboard.sh
rm -f termux-build-final.sh
rm -f termux-no-clipboard.sh
rm -f termux-ultimate-fix.sh
rm -f termux-final-fix.sh
rm -f clean-termux-build.sh
rm -f fix-chat-cli-clipboard.sh
rm -f fix-termux-errors.sh
rm -f fix-specific-errors.sh
rm -f fix-syntax-error.sh
rm -f fix-type-errors.sh
rm -f fix-delimiter-error.sh
rm -f final-termux-fix.sh
rm -f simple-termux-build.sh
rm -f termux-config.toml
rm -f Cargo-android.toml
rm -f fix-compilation.patch

# Remove fake arboard directory if it exists
rm -rf fake-arboard

# Keep only the essential files for Termux build
echo "Keeping only essential Termux build files..."

# Stage the cleanup
git add -A

# Create a single commit with all Termux compatibility changes
git commit -m "Add Termux/Android compatibility for Amazon Q Developer CLI

- Replace arboard clipboard dependency with Android-compatible stub
- Add conditional compilation for Android-specific code paths
- Enable building on Termux environment

🤖 Generated with Claude Code

Co-Authored-By: Claude <noreply@anthropic.com>"

echo "Cleanup complete and changes committed!"