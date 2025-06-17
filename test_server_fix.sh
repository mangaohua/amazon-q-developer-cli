#!/bin/bash

echo "Testing Amazon Q Server OpenAI API compatibility fixes..."

# Test 1: Simple string content
echo "Test 1: Simple string content"
response1=$(curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
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

if echo "$response1" | jq -e '.choices[0].message.content' > /dev/null 2>&1; then
    echo "✅ Test 1 PASSED: Simple string content works"
else
    echo "❌ Test 1 FAILED: Simple string content failed"
    echo "Response: $response1"
fi

# Test 2: Array content (the problematic case)
echo "Test 2: Array content format"
response2=$(curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
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
    ]
  }')

if echo "$response2" | jq -e '.choices[0].message.content' > /dev/null 2>&1; then
    echo "✅ Test 2 PASSED: Array content format works"
else
    echo "❌ Test 2 FAILED: Array content format failed"
    echo "Response: $response2"
fi

# Test 3: Conversation with history
echo "Test 3: Conversation with history"
response3=$(curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
  -H 'Content-Type: application/json' \
  -d '{
    "model": "amazon-q",
    "messages": [
      {
        "role": "user",
        "content": "What is 2+2?"
      },
      {
        "role": "assistant",
        "content": "2+2 equals 4."
      },
      {
        "role": "user",
        "content": "What about 3+3?"
      }
    ]
  }')

if echo "$response3" | jq -e '.choices[0].message.content' > /dev/null 2>&1; then
    echo "✅ Test 3 PASSED: Conversation with history works"
else
    echo "❌ Test 3 FAILED: Conversation with history failed"
    echo "Response: $response3"
fi

echo "Testing completed!"
