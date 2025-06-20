# Kimi Provider Tool Choice 修复

## 🚨 问题描述

使用小米 Kimi 模型时遇到以下错误：

```
Amazon Q is having trouble responding right now:
   0: OpenAI API returned error 400 Bad Request: {"object":"error","message":"\"auto\" tool choice requires --enable-auto-tool-choice and --tool-call-parser to be set","type":"BadRequestError","param":null,"code":400}
```

## 🔍 根本原因

Kimi API 不支持 OpenAI 标准的 `tool_choice: "auto"` 参数，并且需要特定的标志来启用自动工具选择。

### 问题代码
```rust
// 之前的代码
if let Some(tools) = tools {
    if !tools.is_empty() {
        request_body["tools"] = json!(tools);
        request_body["tool_choice"] = json!("auto");  // ❌ Kimi 不支持这个
    }
}
```

## ✅ 解决方案

移除 `tool_choice` 参数，让提供商使用默认的工具选择行为：

### 修复后的代码
```rust
// 修复后的代码
if let Some(tools) = tools {
    if !tools.is_empty() {
        request_body["tools"] = json!(tools);
        // ✅ 不设置 tool_choice，让提供商使用默认行为
        // 大多数提供商会在工具可用时自动使用它们
    }
}
```

## 🔧 技术细节

### API 请求变化

**修复前的请求：**
```json
{
  "model": "kimi-dev",
  "messages": [...],
  "stream": true,
  "tools": [...],
  "tool_choice": "auto"  // ❌ 导致 400 错误
}
```

**修复后的请求：**
```json
{
  "model": "kimi-dev", 
  "messages": [...],
  "stream": true,
  "tools": [...]
  // ✅ 不包含 tool_choice
}
```

### 兼容性考虑

这个修复提高了与不同 OpenAI 兼容提供商的兼容性：

| 提供商 | tool_choice 支持 | 修复后状态 |
|--------|------------------|------------|
| OpenAI | ✅ 支持 "auto" | ✅ 正常工作 |
| Kimi | ❌ 不支持 "auto" | ✅ 修复后正常 |
| Ollama | ⚠️ 部分支持 | ✅ 更好兼容 |
| LocalAI | ⚠️ 取决于版本 | ✅ 更好兼容 |

## 🧪 测试步骤

### 1. 配置 Kimi 提供商
```bash
q settings openai.provider custom
q settings openai.api.baseUrl "http://ms-14376-ev-72b-copy-copy-1-0619142328.kscn-tj5-cloudml.xiaomi.srv/v1"
q settings openai.model kimi-dev
```

### 2. 测试工具调用
```bash
# 测试文件创建工具
q chat "帮我创建一个名为 test.txt 的文件，内容是 'Hello Kimi'"

# 测试其他工具
q chat "显示当前目录的文件列表"
```

### 3. 验证结果
- ✅ 不再出现 400 Bad Request 错误
- ✅ 工具调用功能正常工作
- ✅ 模型可以建议和执行工具

## 📊 影响分析

### 正面影响
- ✅ 修复了 Kimi 提供商的工具调用问题
- ✅ 提高了与其他提供商的兼容性
- ✅ 保持了现有功能的完整性

### 潜在影响
- ⚠️ 某些提供商可能不会自动选择工具（但大多数会）
- ⚠️ 需要依赖提供商的默认工具选择逻辑

### 风险评估
- 🟢 **低风险**：大多数 OpenAI 兼容提供商在有工具可用时会自动使用
- 🟢 **向后兼容**：不影响现有的 Amazon Q 功能
- 🟢 **可回滚**：如果需要可以轻松恢复原来的逻辑

## 🔮 未来改进

如果需要更精细的控制，可以考虑：

### 1. 提供商特定配置
```rust
match provider {
    "openai" => request_body["tool_choice"] = json!("auto"),
    "kimi" => {}, // 不设置 tool_choice
    _ => {}, // 默认不设置
}
```

### 2. 配置选项
添加用户可配置的工具选择策略：
```bash
q settings tool.choice.strategy auto|none|provider-default
```

### 3. 错误重试
如果遇到 tool_choice 相关错误，自动重试不带 tool_choice 的请求。

## 📝 总结

这个修复通过移除可能不兼容的 `tool_choice` 参数，解决了 Kimi 提供商的工具调用问题，同时提高了整体的提供商兼容性。修复是保守和安全的，不会影响现有功能。

现在你可以正常使用 Kimi 模型进行工具调用了！
