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
                        messages.push(json!({
                            "role": "user",
                            "content": user_msg.content
                        }));
                    },
                    crate::api_client::model::ChatMessage::AssistantResponseMessage(assistant_msg) => {
                        messages.push(json!({
                            "role": "assistant", 
                            "content": assistant_msg.content
                        }));
                    },
                }
            }
        }
        
        // Add current user message
        messages.push(json!({
            "role": "user",
            "content": user_input_message.content
        }));

        let request_body = json!({
            "model": openai_client.config.model,
            "messages": messages,
            "stream": true
        });

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
                                    if let Some(content) = delta.get("content").and_then(|v| v.as_str()) {
                                        stream_events.push(ChatResponseStream::AssistantResponseEvent {
                                            content: content.to_string(),
                                        });
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
