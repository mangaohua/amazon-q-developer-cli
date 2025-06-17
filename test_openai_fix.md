# OpenAI 兼容模型修复说明

## 问题描述

您遇到的问题是：即使指定了其他 provider 的模型，系统仍然使用 Amazon Q 的默认模型。

## 根本原因

通过分析代码，我发现了问题的根源：

1. **配置保存正常**：`launch_chat` 函数正确保存了 OpenAI 配置到数据库
2. **客户端创建问题**：`StreamingClient::new()` 方法没有检查 OpenAI 配置，总是创建 Amazon Q 的客户端

## 修复方案

我已经实现了以下修复：

### 1. 更新 `StreamingClient::new()` 方法

```rust
pub async fn new(database: &mut Database) -> Result<Self, ApiClientError> {
    // 检查是否配置了 OpenAI 兼容的 provider
    use crate::cli::chat::openai_config::OpenAiConfig;
    let openai_config = OpenAiConfig::from_database(database);
    
    if openai_config.is_openai_compatible() {
        return Self::new_openai_client(openai_config).await;
    }
    
    // 原有的 Amazon Q 客户端逻辑
    Ok(
        if crate::util::system_info::in_cloudshell()
            || std::env::var("Q_USE_SENDMESSAGE").is_ok_and(|v| !v.is_empty())
        {
            Self::new_qdeveloper_client(database, &Endpoint::load_q(database)).await?
        } else {
            Self::new_codewhisperer_client(database, &Endpoint::load_codewhisperer(database)).await?
        },
    )
}
```

### 2. 添加 OpenAI 客户端支持

- 新增 `OpenAiClient` 结构体
- 实现 `new_openai_client()` 方法
- 添加 `send_openai_message()` 方法处理 OpenAI API 调用
- 更新 `SendMessageOutput` 枚举包含 `OpenAI` 变体

### 3. 实现 OpenAI API 集成

- 将对话历史转换为 OpenAI 格式
- 发送流式请求到 OpenAI API
- 解析 Server-Sent Events 响应
- 转换为内部 `ChatResponseStream` 格式

## 使用方法

修复后，您可以这样使用：

### 设置 Claude-4-Sonnet 为默认模型

```bash
# 设置 provider 为 claude
q settings openai.provider claude

# 设置 API 基础 URL（Anthropic API）
q settings openai.api.baseUrl "https://api.anthropic.com/v1"

# 设置 API 密钥
q settings openai.api.key "your-anthropic-api-key"

# 设置模型
q settings openai.model "claude-3-sonnet-20240229"
```

### 临时使用不同模型

```bash
# 使用 Claude
q chat --provider claude --api-base-url "https://api.anthropic.com/v1" --api-key "your-key" --model "claude-3-sonnet-20240229" "Hello!"

# 使用 OpenAI GPT-4
q chat --provider openai --api-key "your-openai-key" --model "gpt-4" "Hello!"

# 使用本地 Ollama
q chat --provider ollama --api-base-url "http://localhost:11434/v1" --model "llama2" "Hello!"
```

### 验证配置

```bash
# 查看当前配置
q settings all --format json | jq '.openai'

# 测试聊天
q chat "测试消息"
```

## 文件修改清单

1. **`./crates/cli/src/api_client/clients/streaming_client.rs`**
   - 添加 `OpenAiClient` 结构体
   - 更新 `StreamingClient::new()` 检查 OpenAI 配置
   - 实现 OpenAI API 调用逻辑
   - 更新 `SendMessageOutput` 枚举

2. **`./crates/cli/src/api_client/error.rs`**
   - 添加 `Other(String)` 错误变体

## 注意事项

1. **API 密钥安全**：确保 API 密钥安全存储，不要在命令历史中暴露
2. **网络连接**：确保能够访问相应的 API 端点
3. **模型名称**：使用正确的模型名称（如 `claude-3-sonnet-20240229` 而不是 `claude-4-sonnet`）

## 测试建议

1. 先使用 `q settings` 命令配置 provider
2. 使用 `q chat` 测试是否使用了正确的模型
3. 检查响应是否来自配置的 provider 而不是 Amazon Q

这个修复应该解决您遇到的问题，让系统正确使用配置的 OpenAI 兼容模型而不是默认的 Amazon Q 模型。
