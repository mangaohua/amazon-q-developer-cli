#!/bin/bash

echo "🔧 Testing Kimi Provider with Fresh Build"
echo "=========================================="

cd /mnt/d/src/amazon-q-developer-cli

# Use the newly compiled binary
CLI_PATH="./target/release/cli"

if [ ! -f "$CLI_PATH" ]; then
    echo "❌ Binary not found at $CLI_PATH"
    exit 1
fi

echo "✅ Using binary: $CLI_PATH"
echo "Binary timestamp: $(stat -c %y $CLI_PATH)"

# Configure Kimi provider
echo ""
echo "🧪 Configuring Kimi provider"
echo "----------------------------"

$CLI_PATH settings openai.provider custom
$CLI_PATH settings openai.api.baseUrl "http://ms-14376-ev-72b-copy-copy-1-0619142328.kscn-tj5-cloudml.xiaomi.srv/v1"
$CLI_PATH settings openai.model kimi-dev

echo "✅ Configuration complete"

# Show current configuration
echo ""
echo "📋 Current Configuration:"
echo "------------------------"
echo "Provider: $($CLI_PATH settings openai.provider)"
echo "Base URL: $($CLI_PATH settings openai.api.baseUrl)"
echo "Model: $($CLI_PATH settings openai.model)"

echo ""
echo "🎯 Ready to test!"
echo "================="
echo "Now try running:"
echo "  $CLI_PATH chat \"你好，请帮我创建一个文件\""
echo ""
echo "The 400 Bad Request error should be fixed now."

echo ""
echo "🔍 What was fixed:"
echo "=================="
echo "✅ Removed 'tool_choice: \"auto\"' from OpenAI API requests"
echo "✅ Now only sends 'tools' array without tool_choice parameter"
echo "✅ This should work with Kimi and other providers that don't support 'auto'"
