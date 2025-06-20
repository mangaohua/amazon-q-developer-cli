#!/bin/bash

echo "🔧 Final Test - Kimi Provider Tool Choice Fix"
echo "============================================="

cd /mnt/d/src/amazon-q-developer-cli

# Use absolute path to ensure we're using the right binary
CLI_PATH="$(pwd)/target/release/cli"

echo "📍 Using CLI binary: $CLI_PATH"
echo "📅 Binary timestamp: $(stat -c %y "$CLI_PATH")"
echo "📏 Binary size: $(stat -c %s "$CLI_PATH") bytes"

# Verify the binary exists and is executable
if [ ! -f "$CLI_PATH" ]; then
    echo "❌ Binary not found!"
    exit 1
fi

if [ ! -x "$CLI_PATH" ]; then
    echo "❌ Binary is not executable!"
    exit 1
fi

echo "✅ Binary verified"

# Test basic functionality first
echo ""
echo "🧪 Testing basic functionality"
echo "------------------------------"
"$CLI_PATH" --version
if [ $? -ne 0 ]; then
    echo "❌ Basic CLI test failed!"
    exit 1
fi
echo "✅ Basic functionality works"

# Configure Kimi provider
echo ""
echo "🔧 Configuring Kimi provider"
echo "----------------------------"
"$CLI_PATH" settings openai.provider custom
"$CLI_PATH" settings openai.api.baseUrl "http://ms-14376-ev-72b-copy-copy-1-0619142328.kscn-tj5-cloudml.xiaomi.srv/v1"
"$CLI_PATH" settings openai.model kimi-dev

echo "✅ Configuration complete"

# Show configuration
echo ""
echo "📋 Current Configuration:"
echo "------------------------"
echo "Provider: $("$CLI_PATH" settings openai.provider)"
echo "Base URL: $("$CLI_PATH" settings openai.api.baseUrl)"
echo "Model: $("$CLI_PATH" settings openai.model)"

echo ""
echo "🎯 Ready for testing!"
echo "===================="
echo ""
echo "The fix has been applied:"
echo "✅ Removed 'tool_choice: \"auto\"' parameter"
echo "✅ Added debug logging for tool requests"
echo "✅ Using fresh binary compiled at $(stat -c %y "$CLI_PATH")"
echo ""
echo "Now test with:"
echo "  RUST_LOG=debug $CLI_PATH chat \"你好，请帮我创建一个文件\""
echo ""
echo "If you still see the 400 error, please:"
echo "1. Check if you have any other q/cli binaries in your PATH"
echo "2. Make sure you're using this exact binary: $CLI_PATH"
echo "3. Try with RUST_LOG=debug to see detailed logs"

# Check if there are other q binaries in PATH
echo ""
echo "🔍 Checking for other q/cli binaries in PATH:"
echo "--------------------------------------------"
which -a q 2>/dev/null || echo "No 'q' command found in PATH"
which -a cli 2>/dev/null || echo "No 'cli' command found in PATH"

echo ""
echo "💡 To ensure you're using the fixed version, always use the full path:"
echo "   $CLI_PATH"
