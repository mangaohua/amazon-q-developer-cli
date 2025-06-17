#!/bin/bash

echo "🔧 设置 Claude-3-Sonnet 为默认模型"
echo "=================================="

# 注意：Claude-4-Sonnet 目前不存在，使用 Claude-3-Sonnet
echo "设置 provider 为 claude..."
q settings openai.provider "claude"

echo "设置 Anthropic API 基础 URL..."
q settings openai.api.baseUrl "https://api.anthropic.com/v1"

echo "设置模型为 Claude-3-Sonnet..."
q settings openai.model "claude-3-sonnet-20240229"

echo ""
echo "⚠️  请手动设置 API 密钥："
echo "q settings openai.api.key \"your-anthropic-api-key-here\""
echo ""

echo "📋 当前配置："
q settings all --format json 2>/dev/null | jq -r '
  "Provider: " + (."openai.provider" // "not set") + "\n" +
  "Base URL: " + (."openai.api.baseUrl" // "not set") + "\n" +
  "Model: " + (."openai.model" // "not set") + "\n" +
  "API Key: " + (if ."openai.api.key" then "[SET]" else "[NOT SET]" end)
'

echo ""
echo "✅ 配置完成！"
echo ""
echo "💡 使用示例："
echo "q chat \"你好，请介绍一下你自己\""
echo ""
echo "🔄 如需重置为 Amazon Q："
echo "q settings openai.provider \"amazon-q\""
