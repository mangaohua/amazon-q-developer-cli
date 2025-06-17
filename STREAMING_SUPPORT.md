# Amazon Q Server Streaming Support

## 概述

Amazon Q Server 现在完全支持 OpenAI 兼容的 streaming 模式，允许客户端实时接收响应内容，而不需要等待完整响应生成完毕。

## 功能特性

### ✅ **完整的 OpenAI API 兼容性**
- 支持 `stream: true/false` 参数
- Server-Sent Events (SSE) 格式
- 正确的 chunk 结构和字段
- 标准的结束标记 `[DONE]`

### ✅ **双模式支持**
- **Non-streaming**: 传统的完整响应模式
- **Streaming**: 实时流式响应模式
- 自动检测 `stream` 参数，默认为 `false`

### ✅ **内容格式兼容**
- 字符串格式: `"content": "Hello"`
- 数组格式: `"content": [{"type": "text", "text": "Hello"}]`
- 两种格式都支持 streaming 和 non-streaming

## API 使用方法

### Non-Streaming 模式 (默认)

```bash
curl -X POST http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "amazon-q",
    "messages": [
      {
        "role": "user",
        "content": "Hello"
      }
    ]
  }'
```

**响应格式**:
```json
{
  "id": "chatcmpl-xxx",
  "object": "chat.completion",
  "created": 1750145000,
  "model": "amazon-q",
  "choices": [{
    "index": 0,
    "message": {
      "role": "assistant",
      "content": "Hello! How can I help you?",
      "tool_calls": null,
      "function_call": null
    },
    "finish_reason": "stop"
  }],
  "usage": {
    "prompt_tokens": 0,
    "completion_tokens": 0,
    "total_tokens": 0,
    "completion_tokens_details": null,
    "prompt_tokens_details": null
  },
  "system_fingerprint": null,
  "service_tier": null,
  "prompt_logprobs": null,
  "kv_transfer_params": null
}
```

### Streaming 模式

```bash
curl -X POST http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "amazon-q",
    "messages": [
      {
        "role": "user",
        "content": "Hello"
      }
    ],
    "stream": true
  }'
```

**响应格式** (Server-Sent Events):
```
data: {"id":"chatcmpl-xxx","object":"chat.completion.chunk","created":1750145000,"model":"amazon-q","choices":[{"index":0,"delta":{"role":"assistant","content":"Hello"},"finish_reason":null}],"system_fingerprint":null,"service_tier":null}

data: {"id":"chatcmpl-xxx","object":"chat.completion.chunk","created":1750145000,"model":"amazon-q","choices":[{"index":0,"delta":{"content":"! How"},"finish_reason":null}],"system_fingerprint":null,"service_tier":null}

data: {"id":"chatcmpl-xxx","object":"chat.completion.chunk","created":1750145000,"model":"amazon-q","choices":[{"index":0,"delta":{"content":" can I help?"},"finish_reason":null}],"system_fingerprint":null,"service_tier":null}

data: {"id":"chatcmpl-xxx","object":"chat.completion.chunk","created":1750145000,"model":"amazon-q","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"system_fingerprint":null,"service_tier":null}

data: [DONE]
```

## 技术实现

### Streaming 响应结构

```rust
#[derive(Debug, Serialize)]
struct ChatCompletionChunk {
    id: String,
    object: String,           // "chat.completion.chunk"
    created: u64,
    model: String,
    choices: Vec<ChunkChoice>,
    system_fingerprint: Option<String>,
    service_tier: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChunkChoice {
    index: u32,
    delta: ChunkDelta,
    finish_reason: Option<String>,
}

#[derive(Debug, Serialize)]
struct ChunkDelta {
    role: Option<String>,      // 只在第一个 chunk 中包含
    content: Option<String>,   // 每个 chunk 的内容片段
    tool_calls: Option<serde_json::Value>,
    function_call: Option<serde_json::Value>,
}
```

### 响应头设置

Streaming 响应使用以下 HTTP 头：
```
Content-Type: text/event-stream
Cache-Control: no-cache
Connection: keep-alive
Access-Control-Allow-Origin: *
```

## 客户端集成

### cline 配置

在 cline 中配置 Amazon Q Server：
```json
{
  "baseURL": "http://127.0.0.1:8080/v1",
  "model": "amazon-q",
  "streaming": true
}
```

### JavaScript 示例

```javascript
const response = await fetch('http://127.0.0.1:8080/v1/chat/completions', {
  method: 'POST',
  headers: {
    'Content-Type': 'application/json',
  },
  body: JSON.stringify({
    model: 'amazon-q',
    messages: [{ role: 'user', content: 'Hello' }],
    stream: true
  })
});

const reader = response.body.getReader();
const decoder = new TextDecoder();

while (true) {
  const { done, value } = await reader.read();
  if (done) break;
  
  const chunk = decoder.decode(value);
  const lines = chunk.split('\n');
  
  for (const line of lines) {
    if (line.startsWith('data: ')) {
      const data = line.slice(6);
      if (data === '[DONE]') {
        console.log('Stream completed');
        return;
      }
      
      try {
        const parsed = JSON.parse(data);
        const content = parsed.choices[0]?.delta?.content;
        if (content) {
          console.log('Received:', content);
        }
      } catch (e) {
        // Skip invalid JSON
      }
    }
  }
}
```

### Python 示例

```python
import requests
import json

def stream_chat(message):
    response = requests.post(
        'http://127.0.0.1:8080/v1/chat/completions',
        headers={'Content-Type': 'application/json'},
        json={
            'model': 'amazon-q',
            'messages': [{'role': 'user', 'content': message}],
            'stream': True
        },
        stream=True
    )
    
    for line in response.iter_lines():
        if line:
            line = line.decode('utf-8')
            if line.startswith('data: '):
                data = line[6:]
                if data == '[DONE]':
                    break
                try:
                    chunk = json.loads(data)
                    content = chunk['choices'][0]['delta'].get('content')
                    if content:
                        print(content, end='', flush=True)
                except json.JSONDecodeError:
                    continue
    print()  # New line at end

# 使用示例
stream_chat("Tell me about Python")
```

## 测试验证

运行测试脚本验证功能：
```bash
./test_streaming.sh
```

**测试结果**:
- ✅ Non-streaming 模式正常工作
- ✅ Streaming 模式正常工作  
- ✅ 默认行为是 non-streaming
- ✅ 数组内容格式支持 streaming
- ✅ Server-Sent Events 格式正确
- ✅ OpenAI API 完全兼容

## 性能优势

### Streaming 模式优势
1. **更好的用户体验**: 用户可以立即看到响应开始生成
2. **降低感知延迟**: 不需要等待完整响应
3. **实时交互**: 支持打字机效果和实时显示
4. **更好的错误处理**: 可以在流中间检测到错误

### 向后兼容性
- 现有客户端无需修改即可继续使用
- `stream` 参数默认为 `false`
- 响应格式完全符合 OpenAI API 标准

## 启动服务器

```bash
# 构建项目
cargo build --bin cli --release

# 启动服务器
./target/release/cli server --port 8080

# 带调试日志启动
RUST_LOG=debug ./target/release/cli server --port 8080
```

现在 Amazon Q Server 提供了完整的 streaming 支持，可以与 cline 和其他支持 OpenAI API 的客户端无缝集成！
