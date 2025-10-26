use std::convert::Infallible;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::Arc;

use clap::Args;
use eyre::Result;
use http_body_util::{
    BodyExt,
    Full,
    StreamBody,
    combinators::BoxBody,
};
use hyper::body::{Bytes, Frame, Incoming};
use hyper::header::{CONTENT_TYPE, HeaderName, HeaderValue};
use hyper::service::service_fn;
use hyper::{Method, Request, Response, StatusCode};
use hyper_util::rt::{TokioExecutor, TokioIo};
use hyper_util::server::conn::auto::Builder as HyperServerBuilder;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::signal;
use tokio::sync::{
    Mutex,
    mpsc,
};
use tokio_stream::{
    StreamExt,
    wrappers::ReceiverStream,
};
use tracing::{
    debug,
    trace,
};

use crate::api_client::model::{
    AssistantResponseMessage,
    ChatMessage,
    ConversationState,
    FigDocument,
    Tool,
    ToolInputSchema,
    ToolResult,
    ToolResultStatus,
    ToolSpecification,
    UserInputMessage,
};
use crate::api_client::ModelListResult;
use crate::cli::chat::{
    AssistantMessage,
    AssistantToolUse,
    RecvError,
    RequestMetadata,
    ResponseEvent,
    SendMessageError,
    SendMessageStream,
    ToolUseResult,
    ToolUseResultBlock,
};
use crate::os::Os;
use crate::theme::StyledText;

const ANTHROPIC_VERSION: &str = "2023-06-01";
const REQUEST_ID_HEADER: HeaderName = HeaderName::from_static("x-request-id");
const ANTHROPIC_VERSION_HEADER: HeaderName = HeaderName::from_static("anthropic-version");

/// Start a Claude-compatible REST server.
#[derive(Debug, Args, Clone, Copy, PartialEq)]
pub struct ServeArgs {
    /// Port to bind the REST server to.
    #[arg(long, default_value_t = 9_999)]
    pub port: u16,
}

impl ServeArgs {
    pub async fn execute(self, os: &mut Os) -> Result<std::process::ExitCode> {
        let client = os.client.clone();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), self.port);
        let listener = TcpListener::bind(addr).await?;

        println!(
            "{} Claude-compatible server listening on http://{}",
            StyledText::success("started:"),
            listener.local_addr()?
        );

        let shutdown = signal::ctrl_c();
        tokio::pin!(shutdown);

        loop {
            tokio::select! {
                accept = listener.accept() => {
                    match accept {
                        Ok((stream, _)) => {
                            let client = client.clone();
                            tokio::spawn(async move {
                                let service = service_fn(move |req| {
                                    let client = client.clone();
                                    async move { handle_request(req, client).await }
                                });

                                let io = TokioIo::new(stream);
                                if let Err(err) =
                                    HyperServerBuilder::new(TokioExecutor::new())
                                        .serve_connection_with_upgrades(io, service)
                                        .await
                                {
                                    match err.downcast::<hyper::Error>() {
                                        Ok(hyper_err)
                                            if hyper_err.is_incomplete_message()
                                                || hyper_err.is_closed()
                                                || hyper_err.is_canceled() => {},
                                        Ok(hyper_err) => {
                                            eprintln!(
                                                "{} {hyper_err}",
                                                StyledText::error("server error:")
                                            );
                                        },
                                        Err(other) => {
                                            eprintln!("{} {other}", StyledText::error("server error:"));
                                        },
                                    }
                                }
                            });
                        },
                        Err(err) => {
                            eprintln!(
                                "{} failed to accept connection: {err}",
                                StyledText::error("error:")
                            );
                        },
                    }
                },
                _ = &mut shutdown => {
                    println!("{} shutting down", StyledText::info("received ctrl+c:"));
                    break;
                },
            }
        }

        Ok(std::process::ExitCode::SUCCESS)
    }
}

type ApiClient = crate::api_client::ApiClient;

type HandlerResult = Result<Response<BoxBody<Bytes, Infallible>>, Infallible>;

async fn handle_request(req: Request<Incoming>, client: ApiClient) -> HandlerResult {
    let response = match (req.method(), req.uri().path()) {
        (&Method::POST, "/v1/messages") => match handle_messages(req, client).await {
            Ok(resp) => resp,
            Err(err) => err.into_response(),
        },
        _ => ServeError::new(StatusCode::NOT_FOUND, "Endpoint not found").into_response(),
    };

    Ok(response)
}

async fn handle_streaming_response(
    request: AnthropicMessageRequest,
    resolved_model_id: String,
    metadata_lock: Arc<Mutex<Option<RequestMetadata>>>,
    mut stream: SendMessageStream,
    initial_request_id: Option<String>,
) -> Result<Response<BoxBody<Bytes, Infallible>>, ServeError> {
    let (tx, rx) = mpsc::channel::<Result<Bytes, Infallible>>(16);
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, HeaderValue::from_static("text/event-stream"))
        .header(HeaderName::from_static("cache-control"), HeaderValue::from_static("no-cache"))
        .header(HeaderName::from_static("connection"), HeaderValue::from_static("keep-alive"))
        .header(ANTHROPIC_VERSION_HEADER, HeaderValue::from_static(ANTHROPIC_VERSION));

    if let Some(id) = initial_request_id.as_ref() {
        if let Ok(value) = HeaderValue::from_str(id) {
            builder = builder.header(REQUEST_ID_HEADER, value);
        }
    }
    let message_id = initial_request_id
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

    let body = StreamBody::new(
        ReceiverStream::new(rx).map(|chunk| chunk.map(Frame::data)),
    );
    let response = builder
        .body(BoxBody::new(body))
        .map_err(|err| ServeError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    let stream_request = request.clone();
    let resolved_model_clone = resolved_model_id.clone();

    tokio::spawn(async move {
        let mut aggregated_text = String::new();
        let mut aggregated_tool_uses: Vec<AssistantToolUse> = Vec::new();
        let mut latest_metadata: Option<RequestMetadata> = None;
        let mut final_message: Option<AssistantMessage> = None;
        let mut text_block_index: Option<usize> = None;
        let mut next_block_index: usize = 0;
        let mut emitted_text_len: usize = 0;

        debug!(
            %message_id,
            "starting streaming response for Claude-compatible endpoint"
        );

        if send_sse(
            &tx,
            "message_start",
            &serde_json::json!({
                "type": "message_start",
                "message": {
                    "id": message_id.clone(),
                    "type": "message",
                    "role": "assistant",
                    "model": resolved_model_clone,
                    "content": []
                }
            }),
        )
        .await
        .is_err()
        {
            return;
        }

        while let Some(event) = stream.recv().await {
            match event {
                Ok(ResponseEvent::AssistantText(text)) => {
                    aggregated_text.push_str(&text);
                    let new_text = aggregated_text
                        .get(emitted_text_len..)
                        .unwrap_or_default()
                        .to_string();
                    emitted_text_len = aggregated_text.len();
                    if new_text.is_empty() {
                        continue;
                    }

                    trace!(
                        %message_id,
                        delta_len = new_text.len(),
                        total_len = aggregated_text.len(),
                        "streaming text delta"
                    );

                    let index = if let Some(idx) = text_block_index {
                        idx
                    } else {
                        let idx = next_block_index;
                        if send_sse(
                            &tx,
                            "content_block_start",
                            &serde_json::json!({
                                "type": "content_block_start",
                                "index": idx,
                                "content_block": {
                                    "type": "text"
                                }
                            }),
                        )
                        .await
                        .is_err()
                        {
                            return;
                        }
                        text_block_index = Some(idx);
                        next_block_index += 1;
                        idx
                    };

                    if send_sse(
                        &tx,
                        "content_block_delta",
                        &serde_json::json!({
                            "type": "content_block_delta",
                                "index": index,
                                "delta": {
                                    "type": "text_delta",
                                    "text": new_text
                                }
                            }),
                    )
                    .await
                    .is_err()
                    {
                        return;
                    }

                },
                Ok(ResponseEvent::ToolUseStart { .. }) => {},
                Ok(ResponseEvent::ToolUse(tool_use)) => {
                    aggregated_tool_uses.push(tool_use.clone());
                    debug!(
                        %message_id,
                        tool_id = tool_use.id,
                        tool_name = tool_use.name,
                        "streaming tool_use block"
                    );
                    if let Some(idx) = text_block_index.take() {
                        let _ = send_sse(
                            &tx,
                            "content_block_stop",
                            &serde_json::json!({
                                "type": "content_block_stop",
                                "index": idx
                            }),
                        )
                        .await;
                    }

                    let current_index = next_block_index;
                    next_block_index += 1;

                    if send_sse(
                        &tx,
                        "content_block_start",
                        &serde_json::json!({
                            "type": "content_block_start",
                            "index": current_index,
                            "content_block": {
                                "type": "tool_use",
                                "id": tool_use.id,
                                "name": if tool_use.orig_name.is_empty() { tool_use.name.clone() } else { tool_use.orig_name.clone() },
                                "input": tool_use.args
                            }
                        }),
                    )
                    .await
                    .is_err()
                    {
                        return;
                    }

                    if send_sse(
                        &tx,
                        "content_block_stop",
                        &serde_json::json!({
                            "type": "content_block_stop",
                            "index": current_index
                        }),
                    )
                    .await
                    .is_err()
                    {
                        return;
                    }

                    if let Some(summary) = format_tool_use_summary(&tool_use) {
                        let idx = if let Some(idx) = text_block_index {
                            idx
                        } else {
                            let idx = next_block_index;
                            if send_sse(
                                &tx,
                                "content_block_start",
                                &serde_json::json!({
                                    "type": "content_block_start",
                                    "index": idx,
                                    "content_block": { "type": "text" }
                                }),
                            )
                            .await
                            .is_err()
                            {
                                return;
                            }
                            text_block_index = Some(idx);
                            next_block_index += 1;
                            idx
                        };

                        aggregated_text.push_str(&summary);
                        if send_sse(
                            &tx,
                            "content_block_delta",
                            &serde_json::json!({
                                "type": "content_block_delta",
                                "index": idx,
                                "delta": {
                                    "type": "text_delta",
                                    "text": summary
                                }
                            }),
                        )
                        .await
                        .is_err()
                        {
                            return;
                        }
                    }
                },
                Ok(ResponseEvent::EndStream {
                    message,
                    request_metadata,
                }) => {
                    final_message = Some(message);
                    trace!(%message_id, "received EndStream event from backend");
                    latest_metadata = Some(request_metadata);
                    break;
                },
                Err(err) => {
                    debug!(
                        %message_id,
                        error = %err,
                        "streaming backend returned error event"
                    );
                    let _ = send_sse(
                        &tx,
                        "error",
                        &serde_json::json!({
                            "type": "error",
                            "error": {
                                "type": "stream_error",
                                "message": err.to_string()
                            }
                        }),
                    )
                    .await;
                    // signal completion to consumer so parser does not retry endlessly
                    break;
                },
            }
        }

        let Some(final_message) = final_message else {
            debug!(%message_id, "stream ended without final message");
            let _ = send_sse(
                &tx,
                "error",
                &serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "stream_error",
                        "message": "model response stream ended unexpectedly"
                    }
                }),
            )
            .await;
            return;
        };

        let Some(metadata) = latest_metadata else {
            debug!(%message_id, "stream ended without metadata");
            let _ = send_sse(
                &tx,
                "error",
                &serde_json::json!({
                    "type": "error",
                    "error": {
                        "type": "stream_error",
                        "message": "missing response metadata"
                    }
                }),
            )
            .await;
            return;
        };
        let response_model = if stream_request.model.eq_ignore_ascii_case("auto") {
            metadata
                .model_id
                .clone()
                .unwrap_or_else(|| resolved_model_id.clone())
        } else {
            stream_request.model.clone()
        };

        let mut metadata = metadata;
        metadata.message_id = message_id.clone();

        let mut enriched_message = final_message.clone();
        if aggregated_text.is_empty() && !final_message.content().is_empty() {
            aggregated_text.push_str(final_message.content());
        }

        if !aggregated_tool_uses.is_empty() {
            enriched_message = AssistantMessage::ToolUse {
                message_id: final_message.message_id().map(str::to_string),
                content: aggregated_text.clone(),
                tool_uses: aggregated_tool_uses.clone(),
            };
        }

        debug!(
            %message_id,
            "final aggregated_text before response: {}",
            enriched_message.content()
        );

        let anthropic_response = match build_anthropic_response(&response_model, enriched_message, &metadata) {
            Ok(resp) => resp,
            Err(err) => {
                let _ = send_sse(
                    &tx,
                    "error",
                    &serde_json::json!({
                        "type": "error",
                        "error": {
                            "type": "stream_error",
                            "message": err.message
                        }
                    }),
                )
                .await;
                return;
            },
        };

        if let Some(idx) = text_block_index.take() {
            let _ = send_sse(
                &tx,
                "content_block_stop",
                &serde_json::json!({
                    "type": "content_block_stop",
                    "index": idx
                }),
            )
            .await;
        }

        if send_sse(
            &tx,
            "message_delta",
            &serde_json::json!({
                "type": "message_delta",
                "delta": {
                    "stop_reason": anthropic_response.stop_reason,
                    "stop_sequence": anthropic_response.stop_sequence
                }
            }),
        )
        .await
        .is_err()
        {
            return;
        }

        if send_sse(
            &tx,
            "message_stop",
            &serde_json::json!({ "type": "message_stop" }),
        )
        .await
        .is_err()
        {
            return;
        }

        match send_streaming_final_events(&tx, &anthropic_response).await {
            Ok(()) => {
                debug!(
                    %message_id,
                    content_blocks = anthropic_response.content.len(),
                    stop_reason = anthropic_response.stop_reason,
                    "completed streaming response"
                );
                let mut lock = metadata_lock.lock().await;
                *lock = Some(metadata);
            },
            Err(err) => {
                debug!(%message_id, ?err, "failed to send final message event");
                let _ = send_sse(
                    &tx,
                    "error",
                    &serde_json::json!({
                        "type": "error",
                        "error": {
                            "type": "stream_error",
                            "message": "failed to send final message event"
                        }
                    }),
                )
                .await;
                return;
            },
        }
    });

    Ok(response)
}

async fn send_streaming_final_events(
    tx: &mpsc::Sender<Result<Bytes, Infallible>>,
    response: &AnthropicMessageResponse,
) -> Result<(), ()> {
    trace!(
        "sending final message_delta with stop_reason={:?} stop_sequence={:?}",
        response.stop_reason,
        response.stop_sequence
    );
    send_sse(
        tx,
        "message_delta",
        &serde_json::json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": response.stop_reason,
                "stop_sequence": response.stop_sequence
            }
        }),
    )
    .await?;

    trace!("sending message_stop event");
    send_sse(tx, "message_stop", &serde_json::json!({ "type": "message_stop" })).await?;

    trace!("sending final message payload");
    send_sse(tx, "message", &serde_json::to_value(response).unwrap_or(serde_json::Value::Null)).await
}

fn format_tool_use_summary(tool: &AssistantToolUse) -> Option<String> {
    if !tool.name.eq_ignore_ascii_case("TodoWrite") {
        return None;
    }

    let todos = tool
        .args
        .get("todos")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    if todos.is_empty() {
        return Some("\nTodoWrite: no todo items provided.\n".to_string());
    }

    let mut lines = vec!["\nTodoWrite updated the following tasks:".to_string()];
    for item in todos {
        if let Some(obj) = item.as_object() {
            let content = obj
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or_default();
            let status = obj
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let active_form = obj
                .get("activeForm")
                .and_then(|v| v.as_str())
                .unwrap_or_default();

            if active_form.is_empty() {
                lines.push(format!("- [{status}] {content}"));
            } else {
                lines.push(format!("- [{status}] {content} ({active_form})"));
            }
        }
    }
    lines.push(String::new());
    let summary = lines.join("\n");
    debug!(
        tool_use_id = tool.id,
        tool_name = tool.name,
        "generated todo summary: {}",
        summary
    );
    Some(summary)
}

fn build_success_response(
    response: AnthropicMessageResponse,
    request_id: Option<String>,
) -> Result<Response<BoxBody<Bytes, Infallible>>, ServeError> {
    let body = serde_json::to_vec(&response)?;
    let mut builder = Response::builder()
        .status(StatusCode::OK)
        .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
        .header(ANTHROPIC_VERSION_HEADER, HeaderValue::from_static(ANTHROPIC_VERSION));

    if let Some(request_id) = request_id {
        if let Ok(value) = HeaderValue::from_str(&request_id) {
            builder = builder.header(REQUEST_ID_HEADER, value);
        }
    }

    builder
        .body(Full::new(Bytes::from(body)).map_err(|never| match never {}).boxed())
        .map_err(|err| ServeError::new(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

async fn handle_messages(
    req: Request<Incoming>,
    client: ApiClient,
) -> Result<Response<BoxBody<Bytes, Infallible>>, ServeError> {
    let body_bytes = req
        .into_body()
        .collect()
        .await
        .map_err(|err| ServeError::new(StatusCode::BAD_REQUEST, format!("failed to read request body: {err}")))?
        .to_bytes();

    let request: AnthropicMessageRequest = serde_json::from_slice(&body_bytes)
        .map_err(|err| ServeError::new(StatusCode::BAD_REQUEST, format!("invalid request body: {err}")))?;

    let resolved_model_id = resolve_model_id(&client, &request.model).await?;
    let tools = build_tools(&request.tools)?;
    let conversation = build_conversation_state(&request, &resolved_model_id, &tools)?;

    let metadata_lock = Arc::new(Mutex::new(None));
    let mut stream = SendMessageStream::send_message(&client, conversation, metadata_lock.clone(), None)
        .await
        .map_err(ServeError::from_send_message_error)?;

    let initial_request_id = stream.request_id().map(str::to_string);

    if request.stream {
        return handle_streaming_response(
            request,
            resolved_model_id,
            metadata_lock,
            stream,
            initial_request_id,
        )
        .await;
    }

    let mut final_message = None;
    let mut final_metadata = None;

    while let Some(event) = stream.recv().await {
        match event {
            Ok(ResponseEvent::AssistantText(_))
            | Ok(ResponseEvent::ToolUseStart { .. })
            | Ok(ResponseEvent::ToolUse(_)) => {},
            Ok(ResponseEvent::EndStream {
                message,
                request_metadata,
            }) => {
                final_message = Some(message);
                final_metadata = Some(request_metadata);
                break;
            },
            Err(err) => return Err(ServeError::from_recv_error(err)),
        }
    }

    drop(stream);

    let message = final_message.ok_or_else(|| {
        ServeError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "model response stream ended unexpectedly",
        )
    })?;
    let metadata = final_metadata
        .ok_or_else(|| ServeError::new(StatusCode::INTERNAL_SERVER_ERROR, "missing response metadata"))?;

    let response_model = if request.model.eq_ignore_ascii_case("auto") {
        metadata
            .model_id
            .clone()
            .unwrap_or_else(|| resolved_model_id.clone())
    } else {
        request.model.clone()
    };

    let response = build_anthropic_response(&response_model, message, &metadata)?;
    let response = build_success_response(response, metadata.request_id.clone())?;
    Ok(response)
}

fn build_conversation_state(
    request: &AnthropicMessageRequest,
    resolved_model_id: &str,
    available_tools: &[Tool],
) -> Result<ConversationState, ServeError> {
    if request.messages.is_empty() {
        return Err(ServeError::new(StatusCode::BAD_REQUEST, "messages cannot be empty"));
    }

    let mut history = Vec::new();
    let mut system_prompt = String::new();

    if let Some(system) = request.system.clone() {
        let text = system.into_text()?;
        if !text.trim().is_empty() {
            system_prompt.push_str(text.trim());
        }
    }

    for (index, message) in request.messages.iter().enumerate() {
        let is_last = index == request.messages.len() - 1;
        match message.role {
            AnthropicRole::System => {
                let parts = message.content.clone().into_parts()?;
                if !parts.tool_uses.is_empty() || !parts.tool_results.is_empty() {
                    return Err(ServeError::new(
                        StatusCode::BAD_REQUEST,
                        "system messages cannot include tool content",
                    ));
                }
                let text = parts.text;
                if !text.trim().is_empty() {
                    if !system_prompt.is_empty() {
                        system_prompt.push_str("\n\n");
                    }
                    system_prompt.push_str(text.trim());
                }
            },
            AnthropicRole::User => {
                let mut parts = message.content.clone().into_parts()?;
                if !parts.tool_uses.is_empty() {
                    return Err(ServeError::new(
                        StatusCode::BAD_REQUEST,
                        "user messages cannot include tool use blocks",
                    ));
                }

                let has_user_text = !parts.text.trim().is_empty();

                if !has_user_text && !parts.tool_results.is_empty() {
                    parts.text = render_tool_results_text(&parts.tool_results);
                }

                let tool_result_models: Option<Vec<ToolResult>> = if parts.tool_results.is_empty() {
                    None
                } else {
                    let mut sanitized = Vec::new();
                    for mut result in parts.tool_results.iter().cloned() {
                        if result.content.is_empty() {
                            let placeholder = if matches!(result.status, ToolResultStatus::Success) {
                                "Tool completed but produced no output.".to_string()
                            } else {
                                "Tool reported an error but returned no output.".to_string()
                            };
                            result.content.push(ToolUseResultBlock::Text(placeholder));
                        }
                        sanitized.push(result.into());
                    }

                    if sanitized.is_empty() { None } else { Some(sanitized) }
                };

                let mut user_message = UserInputMessage {
                    content: parts.text,
                    images: None,
                    user_input_message_context: None,
                    user_intent: None,
                    model_id: if is_last {
                        Some(resolved_model_id.to_string())
                    } else {
                        None
                    },
                };

                if !system_prompt.is_empty() && is_last {
                    if has_user_text {
                        if user_message.content.is_empty() {
                            user_message.content = system_prompt.clone();
                        } else {
                            user_message.content =
                                format!("{}\n\n{}", system_prompt, user_message.content);
                        }
                    } else if user_message.content.is_empty() {
                        user_message.content = system_prompt.clone();
                    }
                }

                if let Some(tool_results) = tool_result_models {
                    let mut ctx = user_message.user_input_message_context.unwrap_or_default();
                    ctx.tool_results = Some(tool_results);
                    user_message.user_input_message_context = Some(ctx);
                }

                if is_last && !available_tools.is_empty() {
                    let mut ctx = user_message.user_input_message_context.unwrap_or_default();
                    ctx.tools = Some(available_tools.to_vec());
                    user_message.user_input_message_context = Some(ctx);
                }

                if is_last {
                    return Ok(ConversationState {
                        conversation_id: None,
                        user_input_message: user_message,
                        history: if history.is_empty() { None } else { Some(history) },
                    });
                }

                history.push(ChatMessage::UserInputMessage(user_message));
            },
            AnthropicRole::Assistant => {
                let parts = message.content.clone().into_parts()?;
                if !parts.tool_results.is_empty() {
                    return Err(ServeError::new(
                        StatusCode::BAD_REQUEST,
                        "assistant messages cannot include tool result blocks",
                    ));
                }
                let tool_uses = if parts.tool_uses.is_empty() {
                    None
                } else {
                    Some(parts.tool_uses.iter().cloned().map(Into::into).collect())
                };
                history.push(ChatMessage::AssistantResponseMessage(AssistantResponseMessage {
                    message_id: None,
                    content: parts.text,
                    tool_uses,
                }));
            },
        }
    }

    Err(ServeError::new(
        StatusCode::BAD_REQUEST,
        "the final message must be from the user",
    ))
}

#[derive(Debug)]
struct ParsedAnthropicMessage {
    text: String,
    tool_uses: Vec<AssistantToolUse>,
    tool_results: Vec<ToolUseResult>,
}

enum ParsedContentBlock {
    Text(String),
    ToolUse(AssistantToolUse),
    ToolResult(ToolUseResult),
    Skip,
}

fn parse_content_block(value: serde_json::Value) -> Result<ParsedContentBlock, ServeError> {
    use serde_json::Value;

    match value {
        Value::String(text) => Ok(ParsedContentBlock::Text(text)),
        Value::Null => Ok(ParsedContentBlock::Skip),
        Value::Array(items) => {
            let mut fragments = Vec::new();
            for item in items {
                match parse_content_block(item)? {
                    ParsedContentBlock::Text(text) => fragments.push(text),
                    ParsedContentBlock::Skip => {},
                    other => return Ok(other),
                }
            }
            Ok(ParsedContentBlock::Text(fragments.join("\n")))
        },
        Value::Object(mut map) => {
            let type_field = map
                .get("type")
                .and_then(|v| v.as_str())
                .map(str::to_lowercase);

            match type_field.as_deref() {
                Some("text") | Some("input_text") => {
                    let text = map
                        .remove("text")
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_default();
                    Ok(ParsedContentBlock::Text(text))
                },
                Some("tool_use") => {
                    let id = map
                        .remove("id")
                        .and_then(|v| v.as_str().map(str::to_string))
                        .ok_or_else(|| {
                            ServeError::new(StatusCode::BAD_REQUEST, "tool_use block missing 'id'")
                        })?;
                    let name = map
                        .remove("name")
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_else(|| "tool".to_string());
                    let input = map.remove("input").unwrap_or(serde_json::Value::Null);
                    Ok(ParsedContentBlock::ToolUse(AssistantToolUse {
                        id,
                        name: name.clone(),
                        orig_name: name,
                        args: input.clone(),
                        orig_args: input,
                    }))
                },
                Some("tool_result") => {
                    let tool_use_id = map
                        .remove("tool_use_id")
                        .and_then(|v| v.as_str().map(str::to_string))
                        .ok_or_else(|| {
                            ServeError::new(
                                StatusCode::BAD_REQUEST,
                                "tool_result block missing 'tool_use_id'",
                            )
                        })?;
                    let is_error = map
                        .remove("is_error")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let content_values = map
                        .remove("content")
                        .and_then(|v| v.as_array().cloned())
                        .unwrap_or_default();

                    let mut content_blocks = Vec::new();
                    for value in content_values {
                        content_blocks.push(parse_tool_result_content(value)?);
                    }

                    Ok(ParsedContentBlock::ToolResult(ToolUseResult {
                        tool_use_id,
                        content: content_blocks,
                        status: if is_error {
                            ToolResultStatus::Error
                        } else {
                            ToolResultStatus::Success
                        },
                    }))
                },
                Some("json") => {
                    let json_value = map.remove("json").unwrap_or(serde_json::Value::Null);
                    Ok(ParsedContentBlock::Text(json_value.to_string()))
                },
                _ => {
                    if let Some(Value::String(text)) = map.remove("text") {
                        Ok(ParsedContentBlock::Text(text))
                    } else if !map.is_empty() {
                        Ok(ParsedContentBlock::Text(
                            serde_json::Value::Object(map).to_string(),
                        ))
                    } else {
                        Ok(ParsedContentBlock::Skip)
                    }
                },
            }
        },
        other => Ok(ParsedContentBlock::Text(other.to_string())),
    }
}

fn parse_tool_result_content(value: serde_json::Value) -> Result<ToolUseResultBlock, ServeError> {
    use serde_json::Value;

    match value {
        Value::String(text) => Ok(ToolUseResultBlock::Text(text)),
        Value::Null => Ok(ToolUseResultBlock::Text(String::new())),
        Value::Object(mut map) => {
            let type_field = map
                .get("type")
                .and_then(|v| v.as_str())
                .map(str::to_lowercase);

            match type_field.as_deref() {
                Some("text") | Some("input_text") => {
                    let text = map
                        .remove("text")
                        .and_then(|v| v.as_str().map(str::to_string))
                        .unwrap_or_default();
                    Ok(ToolUseResultBlock::Text(text))
                },
                Some("json") => {
                    let json_value = map.remove("json").unwrap_or(serde_json::Value::Null);
                    Ok(ToolUseResultBlock::Json(json_value))
                },
                _ => {
                    if let Some(text) = map.remove("text").and_then(|v| v.as_str().map(str::to_string))
                    {
                        Ok(ToolUseResultBlock::Text(text))
                    } else if !map.is_empty() {
                        Ok(ToolUseResultBlock::Json(serde_json::Value::Object(map)))
                    } else {
                        Ok(ToolUseResultBlock::Text(String::new()))
                    }
                },
            }
        },
        other => Ok(ToolUseResultBlock::Json(other)),
    }
}


fn render_tool_results_text(results: &[ToolUseResult]) -> String {
    let mut parts = Vec::new();
    for result in results {
        for block in &result.content {
            match block {
                ToolUseResultBlock::Text(text) => {
                    if !text.trim().is_empty() {
                        parts.push(text.trim().to_string());
                    }
                },
                ToolUseResultBlock::Json(json) => {
                    if let Ok(serialized) = serde_json::to_string(json) {
                        if !serialized.trim().is_empty() {
                            parts.push(serialized);
                        }
                    }
                },
            }
        }
    }

    if parts.is_empty() {
        "<tool results provided>".to_string()
    } else {
        parts.join(" ")
    }
}

fn build_anthropic_response(
    response_model: &str,
    message: AssistantMessage,
    metadata: &RequestMetadata,
) -> Result<AnthropicMessageResponse, ServeError> {
    let mut content = Vec::new();
    let text = message.content().to_string();
    if !text.trim().is_empty() {
        content.push(AnthropicResponseContent::Text { text });
    }

    if let Some(tool_uses) = message.tool_uses() {
        for tool in tool_uses {
            content.push(AnthropicResponseContent::ToolUse {
                id: tool.id.clone(),
                name: if tool.orig_name.is_empty() {
                    tool.name.clone()
                } else {
                    tool.orig_name.clone()
                },
                input: tool.args.clone(),
            });
        }
    }

    let stop_reason = if message.tool_uses().is_some_and(|tools| !tools.is_empty()) {
        "tool_use"
    } else {
        "end_turn"
    };

    Ok(AnthropicMessageResponse {
        id: message
            .message_id()
            .map(|id| id.to_string())
            .unwrap_or_else(|| metadata.message_id.clone()),
        type_field: "message",
        role: "assistant",
        model: metadata.model_id.clone().unwrap_or_else(|| response_model.to_string()),
        stop_reason,
        stop_sequence: None,
        usage: AnthropicUsage {
            input_tokens: saturating_usize_to_u32(metadata.user_prompt_length),
            output_tokens: saturating_usize_to_u32(metadata.response_size),
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
        },
        content,
    })
}

#[derive(Debug, Deserialize, Clone)]
struct AnthropicMessageRequest {
    model: String,
    #[serde(default)]
    system: Option<AnthropicSystemPrompt>,
    messages: Vec<AnthropicMessage>,
    #[serde(default)]
    tools: Vec<AnthropicTool>,
    #[serde(default)]
    stream: bool,
}

#[derive(Debug, Deserialize, Clone)]
struct AnthropicMessage {
    role: AnthropicRole,
    #[serde(default)]
    content: AnthropicMessageContent,
}

#[derive(Debug, Deserialize, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
enum AnthropicRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum AnthropicSystemPrompt {
    Text(String),
    Blocks(Vec<serde_json::Value>),
}

impl AnthropicSystemPrompt {
    fn into_text(self) -> Result<String, ServeError> {
        match self {
            Self::Text(text) => Ok(text),
            Self::Blocks(blocks) => AnthropicMessageContent::Blocks(blocks).into_text_only(),
        }
    }
}

#[derive(Debug, Deserialize, Clone)]
#[serde(untagged)]
enum AnthropicMessageContent {
    Text(String),
    Blocks(Vec<serde_json::Value>),
}

impl Default for AnthropicMessageContent {
    fn default() -> Self {
        Self::Text(String::new())
    }
}

impl AnthropicMessageContent {
    fn into_parts(self) -> Result<ParsedAnthropicMessage, ServeError> {
        match self {
            Self::Text(text) => Ok(ParsedAnthropicMessage {
                text,
                tool_uses: Vec::new(),
                tool_results: Vec::new(),
            }),
            Self::Blocks(blocks) => {
                let mut text_parts = Vec::new();
                let mut tool_uses = Vec::new();
                let mut tool_results = Vec::new();
                for value in blocks {
                    match parse_content_block(value)? {
                        ParsedContentBlock::Text(text) => text_parts.push(text),
                        ParsedContentBlock::ToolUse(tool_use) => tool_uses.push(tool_use),
                        ParsedContentBlock::ToolResult(result) => tool_results.push(result),
                        ParsedContentBlock::Skip => {},
                    }
                }
                Ok(ParsedAnthropicMessage {
                    text: text_parts.join("\n"),
                    tool_uses,
                    tool_results,
                })
            },
        }
    }

    fn into_text_only(self) -> Result<String, ServeError> {
        let parts = self.into_parts()?;
        if !parts.tool_uses.is_empty() || !parts.tool_results.is_empty() {
            return Err(ServeError::new(
                StatusCode::BAD_REQUEST,
                "tool content blocks are not supported in this context",
            ));
        }
        Ok(parts.text)
    }
}

#[derive(Debug, Deserialize, Clone)]
struct AnthropicTool {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    input_schema: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct AnthropicMessageResponse {
    id: String,
    #[serde(rename = "type")]
    type_field: &'static str,
    role: &'static str,
    model: String,
    stop_reason: &'static str,
    stop_sequence: Option<String>,
    usage: AnthropicUsage,
    content: Vec<AnthropicResponseContent>,
}

#[derive(Debug, Serialize)]
struct AnthropicUsage {
    input_tokens: u32,
    output_tokens: u32,
    cache_creation_input_tokens: u32,
    cache_read_input_tokens: u32,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnthropicResponseContent {
    Text {
        text: String,
    },
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
}

#[derive(Debug)]
struct ServeError {
    status: StatusCode,
    message: String,
    error_type: &'static str,
    request_id: Option<String>,
}

impl ServeError {
    fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            error_type: match status {
                StatusCode::BAD_REQUEST => "invalid_request_error",
                StatusCode::UNAUTHORIZED => "authentication_error",
                StatusCode::FORBIDDEN => "permission_error",
                StatusCode::NOT_FOUND => "not_found_error",
                _ => "api_error",
            },
            request_id: None,
        }
    }

    fn with_request_id(mut self, request_id: Option<String>) -> Self {
        self.request_id = request_id;
        self
    }

    fn into_response(self) -> Response<BoxBody<Bytes, Infallible>> {
        let payload = serde_json::json!({
            "type": "error",
            "error": {
                "type": self.error_type,
                "message": self.message,
            }
        });

        let body = serde_json::to_vec(&payload).unwrap_or_else(|_| b"{}".to_vec());
        let mut builder = Response::builder()
            .status(self.status)
            .header(CONTENT_TYPE, HeaderValue::from_static("application/json"))
            .header(ANTHROPIC_VERSION_HEADER, HeaderValue::from_static(ANTHROPIC_VERSION));

        if let Some(request_id) = self.request_id {
            if let Ok(value) = HeaderValue::from_str(&request_id) {
                builder = builder.header(REQUEST_ID_HEADER, value);
            }
        }

        builder
            .body(Full::new(Bytes::from(body)).map_err(|never| match never {}).boxed())
            .unwrap_or_else(|_| {
                Response::builder()
                    .status(StatusCode::INTERNAL_SERVER_ERROR)
                    .body(Full::new(Bytes::new()).map_err(|never| match never {}).boxed())
                    .unwrap()
            })
    }
}

impl From<serde_json::Error> for ServeError {
    fn from(err: serde_json::Error) -> Self {
        ServeError::new(StatusCode::INTERNAL_SERVER_ERROR, format!("serialization error: {err}"))
    }
}

impl ServeError {
    fn from_send_message_error(err: SendMessageError) -> Self {
        let status = err
            .status_code()
            .and_then(|code| StatusCode::from_u16(code).ok())
            .unwrap_or(StatusCode::BAD_GATEWAY);
        ServeError::new(status, err.to_string()).with_request_id(err.request_metadata.request_id)
    }

    fn from_recv_error(err: RecvError) -> Self {
        let status = err
            .status_code()
            .and_then(|code| StatusCode::from_u16(code).ok())
            .unwrap_or(StatusCode::BAD_GATEWAY);
        ServeError::new(status, err.to_string()).with_request_id(err.request_metadata.request_id)
    }
}

fn saturating_usize_to_u32(value: usize) -> u32 {
    value.min(u32::MAX as usize) as u32
}

fn select_model_id<'a>(requested: &str, model_list: &'a ModelListResult) -> Option<&'a str> {
    model_list
        .models
        .iter()
        .find(|model| model.model_id().eq_ignore_ascii_case(requested))
        .map(|model| model.model_id())
        .or_else(|| {
            model_list.models.iter().find_map(|model| {
                model
                    .model_name()
                    .and_then(|name| name.eq_ignore_ascii_case(requested).then(|| model.model_id()))
            })
        })
}

async fn resolve_model_id(client: &ApiClient, requested: &str) -> Result<String, ServeError> {
    let trimmed = requested.trim();
    if trimmed.is_empty() {
        return Err(ServeError::new(
            StatusCode::BAD_REQUEST,
            "model field must be a non-empty string",
        ));
    }

    if trimmed.starts_with("anthropic.") || trimmed.contains(':') {
        return Ok(trimmed.to_string());
    }

    let model_list = client.list_available_models_cached().await.map_err(|err| {
        ServeError::new(
            StatusCode::BAD_GATEWAY,
            format!("failed to fetch available models: {err}"),
        )
    })?;

    resolve_model_id_from_list(trimmed, &model_list)
}

fn resolve_model_id_from_list(trimmed: &str, model_list: &ModelListResult) -> Result<String, ServeError> {
    if trimmed.eq_ignore_ascii_case("auto") {
        return Ok(model_list.default_model.model_id().to_string());
    }

    if let Some(model_id) = select_model_id(trimmed, model_list) {
        return Ok(model_id.to_string());
    }

    let default_match = model_list
        .default_model
        .model_name()
        .is_some_and(|name| name.eq_ignore_ascii_case(trimmed));
    if default_match || model_list.default_model.model_id().eq_ignore_ascii_case(trimmed) {
        return Ok(model_list.default_model.model_id().to_string());
    }

    Err(ServeError::new(
        StatusCode::BAD_REQUEST,
        format!(
            "model '{trimmed}' is not available. Use 'auto' or select one returned by 'q chat --list-models'."
        ),
    ))
}

fn build_tools(tools: &[AnthropicTool]) -> Result<Vec<Tool>, ServeError> {
    tools
        .iter()
        .cloned()
        .map(AnthropicTool::into_tool)
        .collect()
}

impl AnthropicTool {
    fn into_tool(self) -> Result<Tool, ServeError> {
        let description = self.description.unwrap_or_default();
        let schema_value = self
            .input_schema
            .unwrap_or_else(|| serde_json::json!({ "type": "object" }));
        let fig_schema: FigDocument = serde_json::from_value(schema_value).map_err(|err| {
            ServeError::new(
                StatusCode::BAD_REQUEST,
                format!("invalid tool input_schema for '{}': {err}", self.name),
            )
        })?;

        Ok(Tool::ToolSpecification(ToolSpecification {
            name: self.name,
            description,
            input_schema: ToolInputSchema { json: Some(fig_schema) },
        }))
    }
}

async fn send_sse(
    tx: &mpsc::Sender<Result<Bytes, Infallible>>,
    event: &str,
    payload: &serde_json::Value,
) -> Result<(), ()> {
    let data = serde_json::to_string(payload).map_err(|_| ())?;
    trace!(
        ?event,
        data_len = data.len(),
        preview = %truncate_for_log(&data),
        "sending SSE event"
    );
    let frame = format!("event: {event}\ndata: {data}\n\n");
    tx.send(Ok(Bytes::from(frame))).await.map_err(|_| ())
}

fn truncate_for_log(value: &str) -> String {
    const MAX: usize = 200;
    if value.len() <= MAX {
        value.to_string()
    } else {
        format!(
            "{}…{}",
            &value[..MAX / 2],
            &value[value.len().saturating_sub(MAX / 2)..]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::api_client::model::Tool;
    use amzn_codewhisperer_client::types::Model;
    use crate::api_client::model::ToolResultContentBlock;
    use serde_json::json;

    #[test]
    fn build_conversation_state_uses_resolved_model() {
        let request = AnthropicMessageRequest {
            model: "claude-3-haiku-20240307".to_string(),
            system: None,
            messages: vec![AnthropicMessage {
                role: AnthropicRole::User,
                content: AnthropicMessageContent::Text("Hello".to_string()),
            }],
            tools: Vec::new(),
            stream: false,
        };

        let conversation =
            build_conversation_state(&request, "anthropic.claude-3-haiku-20240307-v1:0", &[]).unwrap();

        assert_eq!(
            conversation.user_input_message.model_id.as_deref(),
            Some("anthropic.claude-3-haiku-20240307-v1:0")
        );
        assert!(conversation.history.is_none());
    }

    #[test]
    fn select_model_id_matches_model_name() {
        let model = Model::builder()
            .model_id("anthropic.claude-3-haiku-20240307-v1:0")
            .model_name("claude-3-haiku-20240307")
            .build()
            .unwrap();
        let model_list = ModelListResult {
            models: vec![model.clone()],
            default_model: model,
        };

        assert_eq!(
            select_model_id("claude-3-haiku-20240307", &model_list),
            Some("anthropic.claude-3-haiku-20240307-v1:0")
        );
    }

    #[test]
    fn resolve_model_id_from_list_supports_auto() {
        let model = Model::builder()
            .model_id("CLAUDE_SONNET_4_20250514_V1_0")
            .model_name("claude-sonnet-4")
            .build()
            .unwrap();
        let model_list = ModelListResult {
            models: vec![model.clone()],
            default_model: model,
        };

        let resolved = resolve_model_id_from_list("auto", &model_list).unwrap();
        assert_eq!(resolved, "CLAUDE_SONNET_4_20250514_V1_0");
    }

    #[test]
    fn resolve_model_id_from_list_errors_on_unknown_model() {
        let model = Model::builder()
            .model_id("CLAUDE_SONNET_4_20250514_V1_0")
            .model_name("claude-sonnet-4")
            .build()
            .unwrap();
        let model_list = ModelListResult {
            models: vec![model.clone()],
            default_model: model,
        };

        let err = resolve_model_id_from_list("unknown-model", &model_list).unwrap_err();
        assert_eq!(err.status, StatusCode::BAD_REQUEST);
        assert!(err.message.contains("unknown-model"));
    }

    #[test]
    fn build_conversation_state_handles_tool_flow() {
        let request = AnthropicMessageRequest {
            model: "auto".to_string(),
            system: None,
            messages: vec![
                AnthropicMessage {
                    role: AnthropicRole::User,
                    content: AnthropicMessageContent::Text("Run a search".to_string()),
                },
                AnthropicMessage {
                    role: AnthropicRole::Assistant,
                    content: AnthropicMessageContent::Blocks(vec![
                        json!({"type": "text", "text": "Invoking search"}),
                        json!({"type": "tool_use", "id": "tool-1", "name": "search", "input": { "query": "rust" } }),
                    ]),
                },
                AnthropicMessage {
                    role: AnthropicRole::User,
                    content: AnthropicMessageContent::Blocks(vec![json!({
                        "type": "tool_result",
                        "tool_use_id": "tool-1",
                        "content": [{
                            "type": "text",
                            "text": "Search result summary"
                        }],
                        "is_error": false
                    })]),
                },
            ],
            tools: vec![AnthropicTool {
                name: "search".to_string(),
                description: Some("Search tool".to_string()),
                input_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" }
                    },
                    "required": ["query"]
                })),
            }],
            stream: false,
        };

        let tools = build_tools(&request.tools).unwrap();
        let conversation = build_conversation_state(&request, "CLAUDE_SONNET", &tools).unwrap();

        let history = conversation.history.expect("history should exist");
        assert_eq!(history.len(), 2);

        match &history[1] {
            ChatMessage::AssistantResponseMessage(assistant) => {
                let tool_uses = assistant.tool_uses.as_ref().expect("tool uses should be present");
                assert_eq!(tool_uses.len(), 1);
                assert_eq!(tool_uses[0].tool_use_id, "tool-1");
                assert_eq!(tool_uses[0].name, "search");
            },
            _ => panic!("expected assistant response in history"),
        }

        let final_message = &conversation.user_input_message;
        assert_eq!(final_message.model_id.as_deref(), Some("CLAUDE_SONNET"));
        assert_eq!(final_message.content.trim(), "Search result summary");

        let context = final_message
            .user_input_message_context
            .as_ref()
            .expect("tool results context should be present");
        let tool_results = context.tool_results.as_ref().expect("tool results should exist");
        assert_eq!(tool_results.len(), 1);
        assert_eq!(tool_results[0].tool_use_id, "tool-1");
        assert!(
            matches!(tool_results[0].content.first(), Some(ToolResultContentBlock::Text(text)) if text == "Search result summary")
        );
        assert!(matches!(tool_results[0].status, ToolResultStatus::Success));

        let tools_ctx = context.tools.as_ref().expect("tools should be present");
        assert_eq!(tools_ctx.len(), 1);
        match &tools_ctx[0] {
            Tool::ToolSpecification(spec) => {
                assert_eq!(spec.name, "search");
            },
        }
    }

    #[test]
    fn build_conversation_state_does_not_duplicate_system_prompt_for_tool_results_only_message() {
        let request = AnthropicMessageRequest {
            model: "auto".to_string(),
            system: Some(AnthropicSystemPrompt::Text(
                "System instructions should only be sent once.".to_string(),
            )),
            messages: vec![
                AnthropicMessage {
                    role: AnthropicRole::User,
                    content: AnthropicMessageContent::Text("Run a search".to_string()),
                },
                AnthropicMessage {
                    role: AnthropicRole::Assistant,
                    content: AnthropicMessageContent::Blocks(vec![
                        json!({"type": "text", "text": "Invoking search"}),
                        json!({"type": "tool_use", "id": "tool-1", "name": "search", "input": { "query": "rust" } }),
                    ]),
                },
                AnthropicMessage {
                    role: AnthropicRole::User,
                    content: AnthropicMessageContent::Blocks(vec![json!({
                        "type": "tool_result",
                        "tool_use_id": "tool-1",
                        "content": [{
                            "type": "text",
                            "text": "Search result summary"
                        }],
                        "is_error": false
                    })]),
                },
            ],
            tools: vec![AnthropicTool {
                name: "search".to_string(),
                description: Some("Search tool".to_string()),
                input_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "query": { "type": "string" }
                    },
                    "required": ["query"]
                })),
            }],
            stream: false,
        };

        let tools = build_tools(&request.tools).unwrap();
        let conversation = build_conversation_state(&request, "CLAUDE_SONNET", &tools).unwrap();
        let final_message = &conversation.user_input_message;

        assert_eq!(final_message.content.trim(), "Search result summary");
        assert!(
            !final_message
                .content
                .contains("System instructions should only be sent once."),
            "system prompt should not be reintroduced when user message only provided tool results"
        );
    }

    #[test]
    fn message_content_accepts_text_without_type() {
        let parts = AnthropicMessageContent::Blocks(vec![json!({ "text": "hello" })])
            .into_parts()
            .expect("should parse");
        assert_eq!(parts.text, "hello");
        assert!(parts.tool_uses.is_empty());
        assert!(parts.tool_results.is_empty());
    }

    #[tokio::test]
    async fn send_sse_formats_event() {
        let (tx, mut rx) = mpsc::channel(1);
        send_sse(&tx, "test", &json!({"ok": true}))
            .await
            .expect("sse should send");
        let frame = rx.recv().await.expect("frame expected").expect("ok frame");
        assert_eq!(frame, Bytes::from_static(b"event: test\ndata: {\"ok\":true}\n\n"));
    }
}
