# OpenAI Compatible Models Support in Q Chat

## 概述

Amazon Q CLI 现在支持配置和使用其他 OpenAI 兼容的模型提供商，包括 OpenAI、Anthropic Claude、本地模型服务等。

## 功能特性

### ✅ **支持的提供商**
- **OpenAI**: 官方 OpenAI API (GPT-3.5, GPT-4, etc.)
- **自定义提供商**: 任何 OpenAI 兼容的 API 端点
- **本地模型**: Ollama, LocalAI, vLLM 等
- **云服务**: Azure OpenAI, AWS Bedrock (通过兼容层)

### ✅ **配置选项**
- **Provider**: 提供商名称 (openai, claude, ollama, etc.)
- **Base URL**: API 端点 URL
- **API Key**: 认证密钥
- **Model**: 模型名称

### ✅ **使用方式**
- **命令行参数**: 临时使用不同的模型
- **持久化配置**: 保存设置供后续使用
- **环境变量**: 通过环境变量配置

## 使用方法

### 1. 命令行参数方式 (临时使用)

```bash
# 使用 OpenAI GPT-4
q chat --provider openai --api-key "your-openai-key" --model gpt-4 "Hello, world!"

# 使用本地 Ollama 模型
q chat --provider ollama --api-base-url "http://localhost:11434/v1" --model llama2 "Explain quantum computing"

# 使用自定义 API
q chat --provider custom --api-base-url "https://your-api.com/v1" --api-key "your-key" --model "your-model" "Question here"
```

### 2. 持久化配置方式

```bash
# 配置 OpenAI 提供商
q settings openai.provider openai
q settings openai.api.baseUrl "https://api.openai.com/v1"
q settings openai.api.key "your-openai-api-key"
q settings openai.model "gpt-4"

# 之后直接使用
q chat "Hello, world!"
```

### 3. 环境变量方式

```bash
# 设置环境变量
export OPENAI_API_KEY="your-openai-key"
export OPENAI_BASE_URL="https://api.openai.com/v1"
export OPENAI_MODEL="gpt-4"

# 使用配置
q chat --provider openai "Hello, world!"
```

## 配置示例

### OpenAI 官方 API

```bash
# 设置配置
q settings openai.provider "openai"
q settings openai.api.baseUrl "https://api.openai.com/v1"
q settings openai.api.key "sk-your-openai-api-key-here"
q settings openai.model "gpt-4"

# 使用
q chat "Explain machine learning"
```

### Anthropic Claude (通过兼容层)

```bash
# 设置配置
q settings openai.provider "claude"
q settings openai.api.baseUrl "https://api.anthropic.com/v1"
q settings openai.api.key "your-anthropic-key"
q settings openai.model "claude-3-sonnet-20240229"

# 使用
q chat "Write a Python function to sort a list"
```

### 本地 Ollama 模型

```bash
# 设置配置
q settings openai.provider "ollama"
q settings openai.api.baseUrl "http://localhost:11434/v1"
q settings openai.model "llama2"

# 使用 (无需 API key)
q chat "What is the capital of France?"
```

### Azure OpenAI

```bash
# 设置配置
q settings openai.provider "azure"
q settings openai.api.baseUrl "https://your-resource.openai.azure.com/openai/deployments/your-deployment"
q settings openai.api.key "your-azure-key"
q settings openai.model "gpt-4"

# 使用
q chat "Summarize this text: [your text here]"
```

## 配置管理

### 查看当前配置

```bash
# 查看所有设置
q settings all --format json

# 查看特定设置
q settings all --format json | jq '.openai'
```

### 重置配置

```bash
# 重置为 Amazon Q
q settings openai.provider "amazon-q"

# 或删除特定设置
q settings --delete openai.provider
q settings --delete openai.api.key
```

## 支持的模型示例

### OpenAI 模型
- `gpt-3.5-turbo`
- `gpt-4`
- `gpt-4-turbo`
- `gpt-4o`

### 本地模型 (Ollama)
- `llama2`
- `codellama`
- `mistral`
- `phi`

### 自定义模型
- 任何支持 OpenAI Chat Completions API 的模型

## 安全注意事项

### API 密钥管理
```bash
# 不要在命令历史中暴露 API 密钥
# 使用环境变量或配置文件
export OPENAI_API_KEY="your-key"
q chat --provider openai "Hello"

# 或使用配置文件
q settings openai.api.key "$(cat ~/.openai-key)"
```

### 本地存储
- API 密钥存储在本地配置文件中
- 确保配置文件权限正确 (600)
- 考虑使用系统密钥管理器

## 故障排除

### 常见问题

1. **API 密钥错误**
   ```bash
   # 检查密钥是否正确设置
   q settings all --format json | jq '.openai.api.key'
   ```

2. **网络连接问题**
   ```bash
   # 测试 API 端点连接
   curl -H "Authorization: Bearer your-key" https://api.openai.com/v1/models
   ```

3. **模型不存在**
   ```bash
   # 列出可用模型
   curl -H "Authorization: Bearer your-key" https://api.openai.com/v1/models
   ```

### 调试模式

```bash
# 启用详细日志
RUST_LOG=debug q chat --provider openai "test message"
```

## 高级用法

### 批量处理

```bash
# 处理多个文件
for file in *.txt; do
    q chat --provider openai --model gpt-4 "Summarize this file: $(cat $file)" > "${file%.txt}_summary.txt"
done
```

### 脚本集成

```bash
#!/bin/bash
# 智能代码审查脚本

PROVIDER="openai"
MODEL="gpt-4"
API_KEY="your-key"

git diff HEAD~1 | q chat \
    --provider "$PROVIDER" \
    --model "$MODEL" \
    --api-key "$API_KEY" \
    "Review this code diff and provide suggestions:"
```

## 贡献和反馈

如果您遇到问题或有改进建议，请：

1. 检查现有的 GitHub Issues
2. 创建新的 Issue 描述问题
3. 提供详细的错误信息和配置

## 未来计划

- [ ] 支持更多认证方式 (OAuth, JWT)
- [ ] 模型性能监控和统计
- [ ] 自动模型选择和负载均衡
- [ ] 更多预设配置模板

---

现在您可以在 Amazon Q CLI 中使用任何 OpenAI 兼容的模型！享受更灵活的 AI 助手体验。
