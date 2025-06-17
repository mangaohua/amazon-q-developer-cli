#!/bin/bash

echo "Testing Amazon Q Server Cline Compatibility..."

# Test the exact format that cline expects
echo "Testing OpenAI API format compatibility..."

response=$(curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
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

echo "Response received:"
echo "$response" | jq .

# Check for required fields that cline expects
echo ""
echo "Checking required fields for cline compatibility..."

# Check if response has choices array with message
if echo "$response" | jq -e '.choices[0].message.content' > /dev/null 2>&1; then
    echo "✅ message.content field present"
else
    echo "❌ message.content field missing"
fi

# Check if response has tool_calls field (even if null)
if echo "$response" | jq -e '.choices[0].message | has("tool_calls")' > /dev/null 2>&1; then
    echo "✅ message.tool_calls field present"
else
    echo "❌ message.tool_calls field missing"
fi

# Check if response has function_call field (even if null)
if echo "$response" | jq -e '.choices[0].message | has("function_call")' > /dev/null 2>&1; then
    echo "✅ message.function_call field present"
else
    echo "❌ message.function_call field missing"
fi

# Check if response has usage field with detailed structure
if echo "$response" | jq -e '.usage | has("completion_tokens_details")' > /dev/null 2>&1; then
    echo "✅ usage.completion_tokens_details field present"
else
    echo "❌ usage.completion_tokens_details field missing"
fi

# Check if response has system_fingerprint field
if echo "$response" | jq -e 'has("system_fingerprint")' > /dev/null 2>&1; then
    echo "✅ system_fingerprint field present"
else
    echo "❌ system_fingerprint field missing"
fi

# Test with array content format (cline's problematic case)
echo ""
echo "Testing array content format (cline's problematic case)..."

array_response=$(curl -s -X POST http://127.0.0.1:8080/v1/chat/completions \
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

if echo "$array_response" | jq -e '.choices[0].message.content' > /dev/null 2>&1; then
    echo "✅ Array content format works"
    echo "Content: $(echo "$array_response" | jq -r '.choices[0].message.content')"
else
    echo "❌ Array content format failed"
    echo "Response: $array_response"
fi

echo ""
echo "Cline compatibility test completed!"
