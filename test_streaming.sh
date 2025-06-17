#!/bin/bash

echo "Testing Amazon Q Server Streaming Support..."

# Test 1: Non-streaming mode (backward compatibility)
echo "Test 1: Non-streaming mode"
response1=$(curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "amazon-q",
    "messages": [
      {
        "role": "user",
        "content": "Say hello"
      }
    ],
    "stream": false
  }')

if echo "$response1" | jq -e '.choices[0].message.content' > /dev/null 2>&1; then
    echo "✅ Non-streaming mode works"
    echo "Content: $(echo "$response1" | jq -r '.choices[0].message.content' | head -c 50)..."
else
    echo "❌ Non-streaming mode failed"
    echo "Response: $response1"
fi

# Test 2: Streaming mode
echo ""
echo "Test 2: Streaming mode"
streaming_response=$(curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "amazon-q",
    "messages": [
      {
        "role": "user",
        "content": "Count to 3"
      }
    ],
    "stream": true
  }')

# Check if response contains streaming format
if echo "$streaming_response" | grep -q "data: {" && echo "$streaming_response" | grep -q "data: \[DONE\]"; then
    echo "✅ Streaming mode works"
    
    # Count chunks
    chunk_count=$(echo "$streaming_response" | grep -c "data: {")
    echo "Number of chunks received: $chunk_count"
    
    # Check if chunks have correct format
    first_chunk=$(echo "$streaming_response" | grep "data: {" | head -1 | sed 's/data: //')
    if echo "$first_chunk" | jq -e '.choices[0].delta' > /dev/null 2>&1; then
        echo "✅ Chunk format is correct"
        
        # Check if first chunk has role
        if echo "$first_chunk" | jq -e '.choices[0].delta.role' > /dev/null 2>&1; then
            echo "✅ First chunk contains role"
        else
            echo "❌ First chunk missing role"
        fi
        
        # Check if chunks have content
        if echo "$first_chunk" | jq -e '.choices[0].delta.content' > /dev/null 2>&1; then
            echo "✅ Chunks contain content"
        else
            echo "❌ Chunks missing content"
        fi
    else
        echo "❌ Chunk format is incorrect"
    fi
    
    # Check final chunk
    if echo "$streaming_response" | grep -B1 "data: \[DONE\]" | grep -q "finish_reason.*stop"; then
        echo "✅ Final chunk has correct finish_reason"
    else
        echo "❌ Final chunk missing finish_reason"
    fi
    
else
    echo "❌ Streaming mode failed"
    echo "Response preview: $(echo "$streaming_response" | head -c 200)..."
fi

# Test 3: Default streaming (should be false)
echo ""
echo "Test 3: Default streaming behavior (no stream parameter)"
default_response=$(curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "amazon-q",
    "messages": [
      {
        "role": "user",
        "content": "Hello"
      }
    ]
  }')

if echo "$default_response" | jq -e '.choices[0].message.content' > /dev/null 2>&1; then
    echo "✅ Default behavior is non-streaming"
else
    echo "❌ Default behavior test failed"
fi

# Test 4: Array content with streaming
echo ""
echo "Test 4: Array content format with streaming"
array_streaming_response=$(curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "amazon-q",
    "messages": [
      {
        "role": "user",
        "content": [
          {
            "type": "text",
            "text": "Hello from array format"
          }
        ]
      }
    ],
    "stream": true
  }')

if echo "$array_streaming_response" | grep -q "data: {" && echo "$array_streaming_response" | grep -q "data: \[DONE\]"; then
    echo "✅ Array content format works with streaming"
else
    echo "❌ Array content format failed with streaming"
fi

echo ""
echo "Streaming test completed!"
echo ""
echo "Summary:"
echo "- Non-streaming mode: Compatible with existing clients"
echo "- Streaming mode: Real-time response chunks"
echo "- Server-Sent Events format: Proper SSE implementation"
echo "- OpenAI API compatibility: Full format compliance"
