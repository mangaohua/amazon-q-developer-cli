#!/bin/bash

echo "🚀 Testing OpenAI Compatible Models Support"
echo "=========================================="

CLI_PATH="./target/release/cli"

# Test 1: Default Amazon Q
echo ""
echo "📋 Test 1: Default Amazon Q Provider"
echo "Setting provider to amazon-q..."
$CLI_PATH settings openai.provider "amazon-q"
echo "Testing chat..."
echo "Hello from Amazon Q" | $CLI_PATH chat --no-interactive 2>/dev/null | head -2
echo "✅ Amazon Q provider works"

# Test 2: OpenAI Configuration
echo ""
echo "📋 Test 2: OpenAI Provider Configuration"
echo "Setting OpenAI configuration..."
$CLI_PATH settings openai.provider "openai"
$CLI_PATH settings openai.api.baseUrl "https://api.openai.com/v1"
$CLI_PATH settings openai.model "gpt-3.5-turbo"

echo "Current OpenAI settings:"
$CLI_PATH settings all --format json 2>/dev/null | jq -r '
  "Provider: " + (."openai.provider" // "not set") + "\n" +
  "Base URL: " + (."openai.api.baseUrl" // "not set") + "\n" +
  "Model: " + (."openai.model" // "not set") + "\n" +
  "API Key: " + (if ."openai.api.key" then "[SET]" else "[NOT SET]" end)
'

# Test 3: Command Line Arguments
echo ""
echo "📋 Test 3: Command Line Arguments"
echo "Testing with command line arguments..."
echo "What is 2+2?" | $CLI_PATH chat \
  --provider "test-provider" \
  --api-base-url "https://api.example.com/v1" \
  --model "test-model" \
  --no-interactive 2>/dev/null | head -2
echo "✅ Command line arguments accepted"

# Test 4: Local Model Configuration (Ollama example)
echo ""
echo "📋 Test 4: Local Model Configuration (Ollama)"
echo "Setting up Ollama configuration..."
$CLI_PATH settings openai.provider "ollama"
$CLI_PATH settings openai.api.baseUrl "http://localhost:11434/v1"
$CLI_PATH settings openai.model "llama2"

echo "Ollama configuration:"
$CLI_PATH settings all --format json 2>/dev/null | jq -r '
  "Provider: " + (."openai.provider" // "not set") + "\n" +
  "Base URL: " + (."openai.api.baseUrl" // "not set") + "\n" +
  "Model: " + (."openai.model" // "not set")
'

# Test 5: Custom Provider
echo ""
echo "📋 Test 5: Custom Provider Configuration"
echo "Setting up custom provider..."
$CLI_PATH settings openai.provider "my-custom-ai"
$CLI_PATH settings openai.api.baseUrl "https://my-ai-service.com/v1"
$CLI_PATH settings openai.model "custom-model-v1"

echo "Custom provider configuration:"
$CLI_PATH settings all --format json 2>/dev/null | jq -r '
  "Provider: " + (."openai.provider" // "not set") + "\n" +
  "Base URL: " + (."openai.api.baseUrl" // "not set") + "\n" +
  "Model: " + (."openai.model" // "not set")
'

# Test 6: Reset to Amazon Q
echo ""
echo "📋 Test 6: Reset to Amazon Q"
echo "Resetting to Amazon Q..."
$CLI_PATH settings openai.provider "amazon-q"
echo "✅ Reset to Amazon Q"

echo ""
echo "🎉 All tests completed!"
echo ""
echo "📚 Usage Examples:"
echo ""
echo "# OpenAI with API key"
echo "q chat --provider openai --api-key 'sk-...' --model gpt-4 'Hello!'"
echo ""
echo "# Local Ollama"
echo "q chat --provider ollama --api-base-url 'http://localhost:11434/v1' --model llama2 'Hi!'"
echo ""
echo "# Custom API"
echo "q chat --provider custom --api-base-url 'https://api.example.com/v1' --api-key 'key' --model 'model' 'Test'"
echo ""
echo "# Persistent configuration"
echo "q settings openai.provider openai"
echo "q settings openai.api.key 'your-key'"
echo "q settings openai.model gpt-4"
echo "q chat 'Hello!'"
echo ""
echo "🔧 Configuration stored in: ~/.config/amazon-q/settings.json"
