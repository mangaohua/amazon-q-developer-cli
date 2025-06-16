use std::net::SocketAddr;
use std::process::ExitCode;
use std::sync::Arc;

use clap::Args;
use eyre::{Result, WrapErr};
use futures::StreamExt;
use hyper::server::conn::http1;
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use tracing::{debug, error, info, warn};

use crate::api_client::{StreamingClient, model::ConversationState, model::UserInputMessage};
use crate::database::Database;
use crate::util::CliContext;

#[derive(Debug, Args, PartialEq, Eq)]
pub struct ServerArgs {
    /// Port to bind the server to
    #[arg(long, short, default_value = "8080")]
    pub port: u16,
    
    /// Host to bind the server to
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,
    
    /// API key for authentication (optional)
    #[arg(long)]
    pub api_key: Option<String>,
    
    /// Model name to report in API responses
    #[arg(long, default_value = "amazon-q")]
    pub model_name: String,
}

// OpenAI API compatible structures
#[derive(Debug, Deserialize)]
struct ChatCompletionRequest {
    model: String,
    messages: Vec<ChatMessage>,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    stream: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize)]
struct ChatMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChatCompletionResponse {
    id: String,
    object: String,
    created: u64,
    model: String,
    choices: Vec<Choice>,
    usage: Usage,
}

#[derive(Debug, Serialize)]
struct Choice {
    index: u32,
    message: ChatMessage,
    finish_reason: String,
}

#[derive(Debug, Serialize)]
struct Usage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

#[derive(Debug, Serialize)]
struct ModelInfo {
    id: String,
    object: String,
    created: u64,
    owned_by: String,
}

#[derive(Debug, Serialize)]
struct ModelsResponse {
    object: String,
    data: Vec<ModelInfo>,
}

#[derive(Debug, Serialize)]
struct ErrorResponse {
    error: ErrorDetail,
}

#[derive(Debug, Serialize)]
struct ErrorDetail {
    message: String,
    #[serde(rename = "type")]
    error_type: String,
    code: Option<String>,
}

struct ServerState {
    client: StreamingClient,
    model_name: String,
    api_key: Option<String>,
}

impl ServerArgs {
    pub async fn execute(&self, database: &mut Database, _cli_context: &CliContext) -> Result<ExitCode> {
        info!("Starting Amazon Q OpenAI-compatible server...");
        
        // Initialize the streaming client
        let client = StreamingClient::new(database).await
            .wrap_err("Failed to initialize Amazon Q client")?;
        
        let state = Arc::new(Mutex::new(ServerState {
            client,
            model_name: self.model_name.clone(),
            api_key: self.api_key.clone(),
        }));
        
        let addr: SocketAddr = format!("{}:{}", self.host, self.port)
            .parse()
            .wrap_err("Invalid host:port combination")?;
        
        let listener = TcpListener::bind(addr).await
            .wrap_err("Failed to bind to address")?;
        
        info!("🚀 Amazon Q OpenAI-compatible server running on http://{}", addr);
        info!("📖 API Documentation:");
        info!("  • Chat Completions: POST /v1/chat/completions");
        info!("  • List Models: GET /v1/models");
        info!("  • Health Check: GET /health");
        
        if let Some(api_key) = &self.api_key {
            info!("🔐 API Key authentication enabled");
            info!("   Use 'Authorization: Bearer {}' header", api_key);
        } else {
            warn!("⚠️  No API key configured - authentication disabled");
        }
        
        info!("💡 Example usage:");
        info!("   curl -X POST http://{}/v1/chat/completions \\", addr);
        info!("     -H 'Content-Type: application/json' \\");
        if self.api_key.is_some() {
            info!("     -H 'Authorization: Bearer YOUR_API_KEY' \\");
        }
        info!("     -d '{{\"model\":\"{}\",\"messages\":[{{\"role\":\"user\",\"content\":\"Hello!\"}}]}}'", self.model_name);
        
        loop {
            let (stream, _) = listener.accept().await
                .wrap_err("Failed to accept connection")?;
            
            let io = TokioIo::new(stream);
            let state = Arc::clone(&state);
            
            tokio::task::spawn(async move {
                if let Err(err) = http1::Builder::new()
                    .serve_connection(io, service_fn(move |req| {
                        let state = Arc::clone(&state);
                        handle_request(req, state)
                    }))
                    .await
                {
                    error!("Error serving connection: {:?}", err);
                }
            });
        }
    }
}

async fn handle_request(
    req: Request<hyper::body::Incoming>,
    state: Arc<Mutex<ServerState>>,
) -> Result<Response<String>, hyper::Error> {
    let method = req.method();
    let path = req.uri().path();
    
    debug!("Handling {} {}", method, path);
    
    // CORS headers
    let response_builder = Response::builder()
        .header("Access-Control-Allow-Origin", "*")
        .header("Access-Control-Allow-Methods", "GET, POST, OPTIONS")
        .header("Access-Control-Allow-Headers", "Content-Type, Authorization");
    
    // Handle preflight requests
    if method == Method::OPTIONS {
        return Ok(response_builder
            .status(StatusCode::OK)
            .body(String::new())
            .unwrap());
    }
    
    // Check API key if configured
    if let Some(expected_key) = &state.lock().await.api_key {
        if let Some(auth_header) = req.headers().get("authorization") {
            if let Ok(auth_str) = auth_header.to_str() {
                if !auth_str.starts_with("Bearer ") || &auth_str[7..] != expected_key {
                    return Ok(create_error_response(
                        StatusCode::UNAUTHORIZED,
                        "Invalid API key",
                        "invalid_api_key"
                    ));
                }
            } else {
                return Ok(create_error_response(
                    StatusCode::UNAUTHORIZED,
                    "Invalid authorization header",
                    "invalid_request"
                ));
            }
        } else {
            return Ok(create_error_response(
                StatusCode::UNAUTHORIZED,
                "Missing authorization header",
                "invalid_request"
            ));
        }
    }
    
    match (method, path) {
        (&Method::GET, "/health") => {
            Ok(response_builder
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(json!({"status": "healthy", "service": "amazon-q-openai-server"}).to_string())
                .unwrap())
        },
        
        (&Method::GET, "/v1/models") => {
            let state = state.lock().await;
            let models = ModelsResponse {
                object: "list".to_string(),
                data: vec![ModelInfo {
                    id: state.model_name.clone(),
                    object: "model".to_string(),
                    created: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    owned_by: "amazon".to_string(),
                }],
            };
            
            Ok(response_builder
                .status(StatusCode::OK)
                .header("content-type", "application/json")
                .body(serde_json::to_string(&models).unwrap())
                .unwrap())
        },
        
        (&Method::POST, "/v1/chat/completions") => {
            handle_chat_completion(req, state).await
        },
        
        _ => {
            Ok(create_error_response(
                StatusCode::NOT_FOUND,
                "Endpoint not found",
                "not_found"
            ))
        }
    }
}

async fn handle_chat_completion(
    req: Request<hyper::body::Incoming>,
    state: Arc<Mutex<ServerState>>,
) -> Result<Response<String>, hyper::Error> {
    // Parse request body
    let body_bytes = match http_body_util::BodyExt::collect(req.into_body()).await {
        Ok(collected) => collected.to_bytes(),
        Err(e) => {
            error!("Failed to read request body: {}", e);
            return Ok(create_error_response(
                StatusCode::BAD_REQUEST,
                "Failed to read request body",
                "invalid_request"
            ));
        }
    };
    
    let chat_request: ChatCompletionRequest = match serde_json::from_slice(&body_bytes) {
        Ok(req) => req,
        Err(e) => {
            error!("Failed to parse JSON: {}", e);
            return Ok(create_error_response(
                StatusCode::BAD_REQUEST,
                &format!("Invalid JSON: {}", e),
                "invalid_request"
            ));
        }
    };
    
    debug!("Chat completion request: {:?}", chat_request);
    
    // Convert messages to Amazon Q format
    let user_message = if let Some(last_message) = chat_request.messages.last() {
        if last_message.role == "user" {
            last_message.content.clone()
        } else {
            return Ok(create_error_response(
                StatusCode::BAD_REQUEST,
                "Last message must be from user",
                "invalid_request"
            ));
        }
    } else {
        return Ok(create_error_response(
            StatusCode::BAD_REQUEST,
            "No messages provided",
            "invalid_request"
        ));
    };
    
    // Build conversation history
    let mut history = Vec::new();
    for (i, msg) in chat_request.messages.iter().enumerate() {
        if i == chat_request.messages.len() - 1 {
            break; // Skip the last message as it's the current user input
        }
        
        match msg.role.as_str() {
            "user" => {
                history.push(crate::api_client::model::ChatMessage::UserInputMessage(
                    UserInputMessage {
                        content: msg.content.clone(),
                        user_input_message_context: None,
                        user_intent: None,
                        images: None,
                    }
                ));
            },
            "assistant" => {
                history.push(crate::api_client::model::ChatMessage::AssistantResponseMessage(
                    crate::api_client::model::AssistantResponseMessage {
                        message_id: None,
                        content: msg.content.clone(),
                        tool_uses: None,
                    }
                ));
            },
            _ => {
                warn!("Unsupported message role: {}", msg.role);
            }
        }
    }
    
    // Create conversation state
    let conversation_state = ConversationState {
        conversation_id: None,
        user_input_message: UserInputMessage {
            content: user_message,
            user_input_message_context: None,
            user_intent: None,
            images: None,
        },
        history: if history.is_empty() { None } else { Some(history) },
    };
    
    // Send to Amazon Q
    let mut state_guard = state.lock().await;
    let response = match state_guard.client.send_message(conversation_state).await {
        Ok(response) => response,
        Err(e) => {
            error!("Amazon Q API error: {}", e);
            return Ok(create_error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Amazon Q API error: {}", e),
                "api_error"
            ));
        }
    };
    
    // Collect the streaming response
    let mut content = String::new();
    let mut response = response;
    
    loop {
        match response.recv().await {
            Ok(Some(event)) => {
                match event {
                    crate::api_client::model::ChatResponseStream::AssistantResponseEvent { content: text } => {
                        content.push_str(&text);
                    },
                    crate::api_client::model::ChatResponseStream::CodeEvent { content: code } => {
                        content.push_str(&code);
                    },
                    _ => {
                        debug!("Received other event type: {:?}", event);
                    }
                }
            },
            Ok(None) => {
                // Stream ended
                break;
            },
            Err(e) => {
                error!("Stream error: {}", e);
                break;
            }
        }
    }
    
    // Create OpenAI-compatible response
    let completion_response = ChatCompletionResponse {
        id: format!("chatcmpl-{}", uuid::Uuid::new_v4().to_string()),
        object: "chat.completion".to_string(),
        created: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        model: state_guard.model_name.clone(),
        choices: vec![Choice {
            index: 0,
            message: ChatMessage {
                role: "assistant".to_string(),
                content,
            },
            finish_reason: "stop".to_string(),
        }],
        usage: Usage {
            prompt_tokens: 0, // Amazon Q doesn't provide token counts
            completion_tokens: 0,
            total_tokens: 0,
        },
    };
    
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "application/json")
        .header("Access-Control-Allow-Origin", "*")
        .body(serde_json::to_string(&completion_response).unwrap())
        .unwrap())
}

fn create_error_response(status: StatusCode, message: &str, error_type: &str) -> Response<String> {
    let error_response = ErrorResponse {
        error: ErrorDetail {
            message: message.to_string(),
            error_type: error_type.to_string(),
            code: None,
        },
    };
    
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("Access-Control-Allow-Origin", "*")
        .body(serde_json::to_string(&error_response).unwrap())
        .unwrap()
}
