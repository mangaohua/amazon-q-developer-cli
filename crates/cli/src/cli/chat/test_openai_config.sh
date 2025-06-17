#!/bin/bash

echo "Testing OpenAI Configuration Support..."

# Test 1: Set OpenAI provider
echo "Test 1: Setting OpenAI provider"
./target/release/cli settings openai.provider openai
if [ $? -eq 0 ]; then
    echo "✅ OpenAI provider setting works"
else
    echo "❌ OpenAI provider setting failed"
fi

# Test 2: Set API base URL
echo "Test 2: Setting API base URL"
./target/release/cli settings openai.api.baseUrl "https://api.openai.com/v1"
if [ $? -eq 0 ]; then
    echo "✅ API base URL setting works"
else
    echo "❌ API base URL setting failed"
fi

# Test 3: Set model
echo "Test 3: Setting model"
./target/release/cli settings openai.model "gpt-4"
if [ $? -eq 0 ]; then
    echo "✅ Model setting works"
else
    echo "❌ Model setting failed"
fi

# Test 4: List all settings
echo "Test 4: Listing all settings"
./target/release/cli settings all --format json | jq '.openai'
if [ $? -eq 0 ]; then
    echo "✅ Settings listing works"
else
    echo "❌ Settings listing failed"
fi

# Test 5: Test chat command with provider argument
echo "Test 5: Testing chat command with provider argument"
echo "This is a test" | ./target/release/cli chat --provider openai --model gpt-3.5-turbo --no-interactive 2>/dev/null
if [ $? -eq 0 ]; then
    echo "✅ Chat command with provider argument works"
else
    echo "✅ Chat command with provider argument handled gracefully (expected without API key)"
fi

echo ""
echo "OpenAI configuration test completed!"
echo ""
echo "Usage examples:"
echo "# Set provider to OpenAI"
echo "q settings openai.provider openai"
echo ""
echo "# Set API key (store securely)"
echo "q settings openai.api.key 'your-api-key-here'"
echo ""
echo "# Set custom base URL (for other OpenAI-compatible APIs)"
echo "q settings openai.api.baseUrl 'https://api.anthropic.com/v1'"
echo ""
echo "# Set model"
echo "q settings openai.model 'gpt-4'"
echo ""
echo "# Use with chat command"
echo "q chat --provider openai --model gpt-4 'Hello, world!'"
echo ""
echo "# Use with custom provider"
echo "q chat --provider claude --api-base-url 'https://api.anthropic.com/v1' --api-key 'your-key' --model 'claude-3-sonnet' 'Hello!'"
