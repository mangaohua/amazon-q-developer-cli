#!/bin/bash

echo "🔧 Testing Kimi Provider Tool Choice Fix"
echo "========================================"

# Build the project
echo "📦 Building the project..."
source $HOME/.cargo/env
cd /mnt/d/src/amazon-q-developer-cli

if [ ! -f "target/release/cli" ]; then
    echo "❌ Binary not found! Please run the build first."
    exit 1
fi

echo "✅ Binary found!"

# Configure Kimi provider
echo ""
echo "🧪 Configuring Kimi provider"
echo "----------------------------"

./target/release/cli settings openai.provider custom
./target/release/cli settings openai.api.baseUrl "http://ms-14376-ev-72b-copy-copy-1-0619142328.kscn-tj5-cloudml.xiaomi.srv/v1"
./target/release/cli settings openai.model kimi-dev

echo "✅ Kimi provider configured"

# Show current configuration
echo ""
echo "📋 Current Configuration:"
echo "------------------------"
echo "Provider: $(./target/release/cli settings openai.provider)"
echo "Base URL: $(./target/release/cli settings openai.api.baseUrl)"
echo "Model: $(./target/release/cli settings openai.model)"

echo ""
echo "🎯 Fix Applied:"
echo "==============="
echo "✅ Removed 'tool_choice: auto' parameter that was causing 400 error"
echo "✅ Now sends only 'tools' array to let provider handle tool selection"
echo "✅ Should work with Kimi and other providers that don't support 'auto' tool choice"

echo ""
echo "🚀 Ready to test!"
echo "================="
echo "You can now run:"
echo "  ./target/release/cli chat \"帮我创建一个文件\""
echo ""
echo "The tool calling should work without the 400 Bad Request error."

echo ""
echo "🔍 What was changed:"
echo "==================="
echo "Before: request_body[\"tool_choice\"] = json!(\"auto\");"
echo "After:  // Omitted tool_choice for better provider compatibility"
echo ""
echo "This allows each provider to use its default tool selection behavior."
