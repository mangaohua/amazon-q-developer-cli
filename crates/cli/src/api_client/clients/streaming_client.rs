use std::sync::{
    Arc,
    Mutex,
};

use amzn_codewhisperer_streaming_client::Client as CodewhispererStreamingClient;
use amzn_qdeveloper_streaming_client::Client as QDeveloperStreamingClient;
use aws_types::request_id::RequestId;
use tracing::{
    debug,
    error,
};

use super::shared::{
    bearer_sdk_config,
    sigv4_sdk_config,
    stalled_stream_protection_config,
};
use crate::api_client::interceptor::opt_out::OptOutInterceptor;
use crate::api_client::model::{
    ChatResponseStream,
    ConversationState,
};
use crate::api_client::{
    ApiClientError,
    Endpoint,
};
use crate::auth::builder_id::BearerResolver;
use crate::aws_common::{
    UserAgentOverrideInterceptor,
    app_name,
};
use crate::database::{
    AuthProfile,
    Database,
};

mod inner {
    use std::sync::{
        Arc,
        Mutex,
    };

    use amzn_codewhisperer_streaming_client::Client as CodewhispererStreamingClient;
    use amzn_qdeveloper_streaming_client::Client as QDeveloperStreamingClient;

    use crate::api_client::model::ChatResponseStream;
    use crate::cli::chat::openai_config::OpenAiConfig;

    #[derive(Clone, Debug)]
    pub enum Inner {
        Codewhisperer(CodewhispererStreamingClient),
        QDeveloper(QDeveloperStreamingClient),
        OpenAI(OpenAiClient),
        Mock(Arc<Mutex<std::vec::IntoIter<Vec<ChatResponseStream>>>>),
    }

    #[derive(Clone, Debug)]
    pub struct OpenAiClient {
        pub config: OpenAiConfig,
        pub http_client: reqwest::Client,
    }
}

#[derive(Clone, Debug)]
pub struct StreamingClient {
    inner: inner::Inner,
    profile: Option<AuthProfile>,
}

impl StreamingClient {
    pub async fn new(database: &mut Database) -> Result<Self, ApiClientError> {
        // Check if OpenAI-compatible provider is configured
        use crate::cli::chat::openai_config::OpenAiConfig;
        let openai_config = OpenAiConfig::from_database(database);
        
        if openai_config.is_openai_compatible() {
            return Self::new_openai_client(openai_config).await;
        }
        
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

    pub async fn new_openai_client(config: crate::cli::chat::openai_config::OpenAiConfig) -> Result<Self, ApiClientError> {
        let http_client = crate::request::new_client()
            .map_err(|e| ApiClientError::Other(format!("Failed to create HTTP client: {}", e)))?;
        
        let openai_client = inner::OpenAiClient {
            config,
            http_client,
        };
        
        Ok(Self {
            inner: inner::Inner::OpenAI(openai_client),
            profile: None,
        })
    }

    pub fn mock(events: Vec<Vec<ChatResponseStream>>) -> Self {
        Self {
            inner: inner::Inner::Mock(Arc::new(Mutex::new(events.into_iter()))),
            profile: None,
        }
    }

    pub async fn new_codewhisperer_client(
        database: &mut Database,
        endpoint: &Endpoint,
    ) -> Result<Self, ApiClientError> {
        let conf_builder: amzn_codewhisperer_streaming_client::config::Builder =
            (&bearer_sdk_config(database, endpoint).await).into();
        let conf = conf_builder
            .http_client(crate::aws_common::http_client::client())
            .interceptor(OptOutInterceptor::new(database))
            .interceptor(UserAgentOverrideInterceptor::new())
            .bearer_token_resolver(BearerResolver)
            .app_name(app_name())
            .endpoint_url(endpoint.url())
            .stalled_stream_protection(stalled_stream_protection_config())
            .build();
        let inner = inner::Inner::Codewhisperer(CodewhispererStreamingClient::from_conf(conf));

        let profile = match database.get_auth_profile() {
            Ok(profile) => profile,
            Err(err) => {
                error!("Failed to get auth profile: {err}");
                None
            },
        };

        Ok(Self { inner, profile })
    }

    pub async fn new_qdeveloper_client(database: &Database, endpoint: &Endpoint) -> Result<Self, ApiClientError> {
        let conf_builder: amzn_qdeveloper_streaming_client::config::Builder =
            (&sigv4_sdk_config(database, endpoint).await?).into();
        let conf = conf_builder
            .http_client(crate::aws_common::http_client::client())
            .interceptor(OptOutInterceptor::new(database))
            .interceptor(UserAgentOverrideInterceptor::new())
            .app_name(app_name())
            .endpoint_url(endpoint.url())
            .stalled_stream_protection(stalled_stream_protection_config())
            .build();
        let client = QDeveloperStreamingClient::from_conf(conf);
        Ok(Self {
            inner: inner::Inner::QDeveloper(client),
            profile: None,
        })
    }

    pub async fn send_message(
        &self,
        conversation_state: ConversationState,
    ) -> Result<SendMessageOutput, ApiClientError> {
        debug!("Sending conversation: {:#?}", conversation_state);

        match &self.inner {
            inner::Inner::Codewhisperer(client) => {
                let ConversationState {
                    conversation_id,
                    user_input_message,
                    history,
                } = conversation_state;
                
                let conversation_state = amzn_codewhisperer_streaming_client::types::ConversationState::builder()
                    .set_conversation_id(conversation_id)
                    .current_message(
                        amzn_codewhisperer_streaming_client::types::ChatMessage::UserInputMessage(
                            user_input_message.into(),
                        ),
                    )
                    .chat_trigger_type(amzn_codewhisperer_streaming_client::types::ChatTriggerType::Manual)
                    .set_history(
                        history
                            .map(|v| v.into_iter().map(|i| i.try_into()).collect::<Result<Vec<_>, _>>())
                            .transpose()?,
                    )
                    .build()
                    .expect("building conversation_state should not fail");
                let response = client
                    .generate_assistant_response()
                    .conversation_state(conversation_state)
                    .set_profile_arn(self.profile.as_ref().map(|p| p.arn.clone()))
                    .send()
                    .await;

                match response {
                    Ok(resp) => Ok(SendMessageOutput::Codewhisperer(resp)),
                    Err(e) => {
                        let is_quota_breach = e.raw_response().is_some_and(|resp| resp.status().as_u16() == 429);
                        let is_context_window_overflow = e.as_service_error().is_some_and(|err| {
                            matches!(err, err if err.meta().code() == Some("ValidationException")
                                && err.meta().message() == Some("Input is too long."))
                        });

                        if is_quota_breach {
                            Err(ApiClientError::QuotaBreach("quota has reached its limit"))
                        } else if is_context_window_overflow {
                            Err(ApiClientError::ContextWindowOverflow)
                        } else {
                            Err(e.into())
                        }
                    },
                }
            },
            inner::Inner::QDeveloper(client) => {
                let ConversationState {
                    conversation_id,
                    user_input_message,
                    history,
                } = conversation_state;
                
                let conversation_state_builder = amzn_qdeveloper_streaming_client::types::ConversationState::builder()
                    .set_conversation_id(conversation_id)
                    .current_message(amzn_qdeveloper_streaming_client::types::ChatMessage::UserInputMessage(
                        user_input_message.into(),
                    ))
                    .chat_trigger_type(amzn_qdeveloper_streaming_client::types::ChatTriggerType::Manual)
                    .set_history(
                        history
                            .map(|v| v.into_iter().map(|i| i.try_into()).collect::<Result<Vec<_>, _>>())
                            .transpose()?,
                    );

                Ok(SendMessageOutput::QDeveloper(
                    client
                        .send_message()
                        .conversation_state(conversation_state_builder.build().expect("fix me"))
                        .send()
                        .await?,
                ))
            },
            inner::Inner::OpenAI(openai_client) => {
                self.send_openai_message(openai_client, conversation_state).await
            },
            inner::Inner::Mock(events) => {
                let mut new_events = events.lock().unwrap().next().unwrap_or_default().clone();
                new_events.reverse();
                Ok(SendMessageOutput::Mock(new_events))
            },
        }
    }

    async fn send_openai_message(
        &self,
        openai_client: &inner::OpenAiClient,
        conversation_state: ConversationState,
    ) -> Result<SendMessageOutput, ApiClientError> {
        use serde_json::json;
        
        let ConversationState {
            user_input_message,
            history,
            ..
        } = conversation_state;

        // Convert conversation to OpenAI format
        let mut messages = Vec::new();
        
        // Add history messages
        if let Some(history) = history {
            for msg in history {
                match msg {
                    crate::api_client::model::ChatMessage::UserInputMessage(user_msg) => {
                        let mut user_message = json!({
                            "role": "user",
                            "content": user_msg.content
                        });
                        
                        // Add tool results if present
                        if let Some(context) = &user_msg.user_input_message_context {
                            if let Some(tool_results) = &context.tool_results {
                                let mut tool_calls = Vec::new();
                                for tool_result in tool_results {
                                    let content = tool_result.content.iter()
                                        .map(|block| match block {
                                            crate::api_client::model::ToolResultContentBlock::Text(text) => text.clone(),
                                            crate::api_client::model::ToolResultContentBlock::Json(json_val) => {
                                                // Convert AWS Document to string representation
                                                format!("{:?}", json_val)
                                            }
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n");
                                    
                                    tool_calls.push(json!({
                                        "tool_call_id": tool_result.tool_use_id,
                                        "content": content
                                    }));
                                }
                                
                                if !tool_calls.is_empty() {
                                    user_message["tool_calls"] = json!(tool_calls);
                                    user_message["role"] = json!("tool");
                                }
                            }
                        }
                        
                        messages.push(user_message);
                    },
                    crate::api_client::model::ChatMessage::AssistantResponseMessage(assistant_msg) => {
                        // Check if this assistant message contains tool calls
                        // For now, we'll just add it as a regular assistant message
                        // TODO: Parse assistant message for tool calls if needed
                        messages.push(json!({
                            "role": "assistant", 
                            "content": assistant_msg.content
                        }));
                    },
                }
            }
        }
        
        // Add current user message
        let mut current_message = json!({
            "role": "user",
            "content": user_input_message.content
        });
        
        // Add tool results if present in current message
        if let Some(context) = &user_input_message.user_input_message_context {
            if let Some(tool_results) = &context.tool_results {
                let mut tool_calls = Vec::new();
                for tool_result in tool_results {
                    let content = tool_result.content.iter()
                        .map(|block| match block {
                            crate::api_client::model::ToolResultContentBlock::Text(text) => text.clone(),
                            crate::api_client::model::ToolResultContentBlock::Json(json_val) => {
                                // Convert AWS Document to string representation
                                format!("{:?}", json_val)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("\n");
                    
                    tool_calls.push(json!({
                        "tool_call_id": tool_result.tool_use_id,
                        "content": content
                    }));
                }
                
                if !tool_calls.is_empty() {
                    current_message["tool_calls"] = json!(tool_calls);
                    current_message["role"] = json!("tool");
                }
            }
        }
        
        messages.push(current_message);

        // Get available tools from conversation state
        let tools = if let Some(context) = &user_input_message.user_input_message_context {
            if let Some(tools) = &context.tools {
                let mut openai_tools = Vec::new();
                for tool in tools {
                    if let crate::api_client::model::Tool::ToolSpecification(spec) = tool {
                        openai_tools.push(json!({
                            "type": "function",
                            "function": {
                                "name": spec.name,
                                "description": spec.description,
                                "parameters": spec.input_schema.json.as_ref().map(|doc| {
                                    // Convert FigDocument to JSON value
                                    // For now, we'll use a simple object structure
                                    json!({
                                        "type": "object",
                                        "properties": {},
                                        "required": []
                                    })
                                }).unwrap_or_else(|| json!({
                                    "type": "object",
                                    "properties": {},
                                    "required": []
                                }))
                            }
                        }));
                    }
                }
                Some(openai_tools)
            } else {
                None
            }
        } else {
            None
        };

        let mut request_body = json!({
            "model": openai_client.config.model,
            "messages": messages,
            "stream": true
        });

        if let Some(tools) = tools {
            if !tools.is_empty() {
                // Check if this is a Kimi-based API that requires specific tool choice parameters
                if openai_client.config.base_url.contains("xiaomi.srv") {
                    // For Kimi-based APIs, don't send tools to avoid tool_choice requirement
                    debug!("Skipping tools for Kimi-based API to avoid tool_choice requirement");
                } else {
                    request_body["tools"] = json!(tools);
                    // Don't set tool_choice to maintain compatibility with different providers
                    // Most providers will automatically use tools when they're available
                    debug!("Sending {} tools to OpenAI-compatible API without tool_choice parameter", tools.len());
                }
            }
        } else {
            debug!("No tools available for OpenAI-compatible API request");
        }

        let mut request_builder = openai_client.http_client
            .post(&format!("{}/chat/completions", openai_client.config.base_url))
            .header("Content-Type", "application/json")
            .json(&request_body);

        if let Some(api_key) = &openai_client.config.api_key {
            request_builder = request_builder.header("Authorization", format!("Bearer {}", api_key));
        }

        let response = request_builder.send().await
            .map_err(|e| ApiClientError::Other(format!("OpenAI API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(ApiClientError::Other(format!(
                "OpenAI API returned error {}: {}", status, error_text
            )));
        }

        // Convert response to our format
        let response_stream = self.convert_openai_response_stream(response).await?;
        Ok(SendMessageOutput::OpenAI {
            events: response_stream,
            index: 0,
        })
    }

    async fn convert_openai_response_stream(
        &self,
        response: reqwest::Response,
    ) -> Result<Vec<ChatResponseStream>, ApiClientError> {
        use futures::StreamExt;
        
        let mut stream_events = Vec::new();
        let mut stream = response.bytes_stream();
        let mut buffer = String::new();
        let mut current_tool_calls: std::collections::HashMap<usize, serde_json::Value> = std::collections::HashMap::new();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| ApiClientError::Other(format!("Stream error: {}", e)))?;
            let chunk_str = String::from_utf8_lossy(&chunk);
            buffer.push_str(&chunk_str);

            // Process complete lines
            while let Some(line_end) = buffer.find('\n') {
                let line = buffer[..line_end].trim().to_string();
                buffer = buffer[line_end + 1..].to_string();

                if line.starts_with("data: ") {
                    let data = &line[6..];
                    if data == "[DONE]" {
                        break;
                    }
                    
                    if let Ok(json_data) = serde_json::from_str::<serde_json::Value>(data) {
                        if let Some(choices) = json_data.get("choices").and_then(|v| v.as_array()) {
                            if let Some(choice) = choices.first() {
                                if let Some(delta) = choice.get("delta").and_then(|v| v.as_object()) {
                                    // Handle text content
                                    if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                        stream_events.push(ChatResponseStream::AssistantResponseEvent {
                                            content: content.to_string(),
                                        });
                                    }
                                    
                                    // Handle tool calls
                                    if let Some(tool_calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
                                        for tool_call in tool_calls {
                                            if let Some(index) = tool_call.get("index").and_then(|v| v.as_u64()) {
                                                let index = index as usize;
                                                
                                                // Initialize or update the tool call
                                                let entry = current_tool_calls.entry(index).or_insert_with(|| {
                                                    serde_json::json!({
                                                        "id": "",
                                                        "type": "function",
                                                        "function": {
                                                            "name": "",
                                                            "arguments": ""
                                                        }
                                                    })
                                                });
                                                
                                                // Update tool call ID
                                                if let Some(id) = tool_call.get("id").and_then(|v| v.as_str()) {
                                                    entry["id"] = serde_json::Value::String(id.to_string());
                                                }
                                                
                                                // Update function details
                                                if let Some(function) = tool_call.get("function").and_then(|v| v.as_object()) {
                                                    if let Some(name) = function.get("name").and_then(|v| v.as_str()) {
                                                        entry["function"]["name"] = serde_json::Value::String(name.to_string());
                                                        
                                                        // Emit tool use start event
                                                        stream_events.push(ChatResponseStream::ToolUseEvent {
                                                            tool_use_id: entry["id"].as_str().unwrap_or("").to_string(),
                                                            name: name.to_string(),
                                                            input: None,
                                                            stop: None,
                                                        });
                                                    }
                                                    
                                                    if let Some(arguments) = function.get("arguments").and_then(|v| v.as_str()) {
                                                        // Append arguments
                                                        let current_args = entry["function"]["arguments"].as_str().unwrap_or("");
                                                        let new_args = format!("{}{}", current_args, arguments);
                                                        entry["function"]["arguments"] = serde_json::Value::String(new_args.clone());
                                                        
                                                        // Emit tool use event with partial input
                                                        stream_events.push(ChatResponseStream::ToolUseEvent {
                                                            tool_use_id: entry["id"].as_str().unwrap_or("").to_string(),
                                                            name: entry["function"]["name"].as_str().unwrap_or("").to_string(),
                                                            input: Some(arguments.to_string()),
                                                            stop: None,
                                                        });
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                
                                // Check if this is the end of the stream
                                if let Some(finish_reason) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                                    if finish_reason == "tool_calls" {
                                        // Emit final tool use events
                                        for (_, tool_call) in current_tool_calls.iter() {
                                            stream_events.push(ChatResponseStream::ToolUseEvent {
                                                tool_use_id: tool_call["id"].as_str().unwrap_or("").to_string(),
                                                name: tool_call["function"]["name"].as_str().unwrap_or("").to_string(),
                                                input: None,
                                                stop: Some(true),
                                            });
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(stream_events)
    }
}

#[derive(Debug)]
pub enum SendMessageOutput {
    Codewhisperer(
        amzn_codewhisperer_streaming_client::operation::generate_assistant_response::GenerateAssistantResponseOutput,
    ),
    QDeveloper(amzn_qdeveloper_streaming_client::operation::send_message::SendMessageOutput),
    OpenAI {
        events: Vec<ChatResponseStream>,
        index: usize,
    },
    Mock(Vec<ChatResponseStream>),
}

impl SendMessageOutput {
    pub fn request_id(&self) -> Option<&str> {
        match self {
            SendMessageOutput::Codewhisperer(output) => output.request_id(),
            SendMessageOutput::QDeveloper(output) => output.request_id(),
            SendMessageOutput::OpenAI { .. } => Some("<openai-request-id>"),
            SendMessageOutput::Mock(_) => None,
        }
    }

    pub async fn recv(&mut self) -> Result<Option<ChatResponseStream>, ApiClientError> {
        match self {
            SendMessageOutput::Codewhisperer(output) => Ok(output
                .generate_assistant_response_response
                .recv()
                .await?
                .map(|s| s.into())),
            SendMessageOutput::QDeveloper(output) => Ok(output.send_message_response.recv().await?.map(|s| s.into())),
            SendMessageOutput::OpenAI { events, index } => {
                if *index < events.len() {
                    let event = events[*index].clone();
                    *index += 1;
                    Ok(Some(event))
                } else {
                    Ok(None)
                }
            },
            SendMessageOutput::Mock(vec) => Ok(vec.pop()),
        }
    }
}

impl RequestId for SendMessageOutput {
    fn request_id(&self) -> Option<&str> {
        match self {
            SendMessageOutput::Codewhisperer(output) => output.request_id(),
            SendMessageOutput::QDeveloper(output) => output.request_id(),
            SendMessageOutput::OpenAI { .. } => Some("<openai-request-id>"),
            SendMessageOutput::Mock(_) => Some("<mock-request-id>"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api_client::model::{
        AssistantResponseMessage,
        ChatMessage,
        UserInputMessage,
    };

    #[tokio::test]
    async fn create_clients() {
        let mut database = Database::new().await.unwrap();
        let endpoint = Endpoint::load_codewhisperer(&database);

        let _ = StreamingClient::new(&mut database).await;
        let _ = StreamingClient::new_codewhisperer_client(&mut database, &endpoint).await;
        let _ = StreamingClient::new_qdeveloper_client(&database, &endpoint).await;
    }

    #[tokio::test]
    async fn test_mock() {
        let client = StreamingClient::mock(vec![vec![
            ChatResponseStream::AssistantResponseEvent {
                content: "Hello!".to_owned(),
            },
            ChatResponseStream::AssistantResponseEvent {
                content: " How can I".to_owned(),
            },
            ChatResponseStream::AssistantResponseEvent {
                content: " assist you today?".to_owned(),
            },
        ]]);
        let mut output = client
            .send_message(ConversationState {
                conversation_id: None,
                user_input_message: UserInputMessage {
                    images: None,
                    content: "Hello".into(),
                    user_input_message_context: None,
                    user_intent: None,
                },
                history: None,
            })
            .await
            .unwrap();

        let mut output_content = String::new();
        while let Some(ChatResponseStream::AssistantResponseEvent { content }) = output.recv().await.unwrap() {
            output_content.push_str(&content);
        }
        assert_eq!(output_content, "Hello! How can I assist you today?");
    }

    #[ignore]
    #[tokio::test]
    async fn assistant_response() {
        let mut database = Database::new().await.unwrap();
        let client = StreamingClient::new(&mut database).await.unwrap();
        let mut response = client
            .send_message(ConversationState {
                conversation_id: None,
                user_input_message: UserInputMessage {
                    images: None,
                    content: "How about rustc?".into(),
                    user_input_message_context: None,
                    user_intent: None,
                },
                history: Some(vec![
                    ChatMessage::UserInputMessage(UserInputMessage {
                        images: None,
                        content: "What language is the linux kernel written in, and who wrote it?".into(),
                        user_input_message_context: None,
                        user_intent: None,
                    }),
                    ChatMessage::AssistantResponseMessage(AssistantResponseMessage {
                        content: "It is written in C by Linus Torvalds.".into(),
                        message_id: None,
                        tool_uses: None,
                    }),
                ]),
            })
            .await
            .unwrap();

        while let Some(event) = response.recv().await.unwrap() {
            println!("{:?}", event);
        }
    }
}
