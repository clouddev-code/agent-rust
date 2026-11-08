//! Bedrock ConverseStream を使ったツール呼び出しループ。
//!
//! モデルの出力を受け取りながら AG-UI イベントに変換して送る。
//! `stopReason` が `tool_use` の間はツールを実行して結果を履歴に足し、
//! もう一度モデルを呼ぶ。

use anyhow::{Context, Result, anyhow, bail};
use aws_sdk_bedrockruntime::error::DisplayErrorContext;
use aws_sdk_bedrockruntime::types::{
    ContentBlock, ContentBlockDelta, ContentBlockStart, ConversationRole, ConverseStreamOutput,
    InferenceConfiguration, Message, StopReason, SystemContentBlock, ToolResultBlock, ToolResultContentBlock,
    ToolResultStatus, ToolUseBlock,
};
use serde_json::{Value, json};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::AppState;
use crate::agui::{Event, RunAgentInput};
use crate::convert::{self, json_to_document};

/// モデル呼び出しの上限回数。ツール呼び出しが止まらない場合の歯止め。
const MAX_MODEL_CALLS: usize = 8;
const MAX_TOKENS: i32 = 4096;

const SYSTEM_PROMPT: &str = "\
あなたは親切なアシスタントです。ユーザーの質問に日本語で簡潔に答えてください。
必要な場合だけツールを使ってください。get_sample_metrics の値はデモ用のダミーデータです。";

/// イベントの送り先。フロントエンドとの接続が切れたら送信エラーにしてループを止める。
struct Emitter(mpsc::Sender<Event>);

impl Emitter {
    async fn emit(&self, event: Event) -> Result<()> {
        self.0.send(event).await.map_err(|_| anyhow!("client disconnected"))
    }
}

/// 1 回の実行（AG-UI の run）を処理し、イベントを `tx` に送る。
/// `RUN_STARTED` から `RUN_FINISHED` / `RUN_ERROR` までをここで送る。
pub async fn run(state: AppState, input: RunAgentInput, tx: mpsc::Sender<Event>) {
    let emitter = Emitter(tx);
    let started = Event::RunStarted {
        thread_id: input.thread_id.clone(),
        run_id: input.run_id.clone(),
    };
    if emitter.emit(started).await.is_err() {
        return;
    }

    let last = match run_model_loop(&state, &input, &emitter).await {
        Ok(()) => Event::RunFinished {
            thread_id: input.thread_id,
            run_id: input.run_id,
        },
        Err(error) => {
            tracing::error!(error = format!("{error:#}"), "run failed");
            Event::RunError {
                message: format!("{error:#}"),
            }
        }
    };
    // ここで失敗するのは接続が切れた場合だけなので無視する。
    let _ = emitter.emit(last).await;
}

async fn run_model_loop(state: &AppState, input: &RunAgentInput, emitter: &Emitter) -> Result<()> {
    let conversation = convert::to_converse(&input.messages).context("invalid message history")?;
    if conversation.messages.is_empty() {
        bail!("no user message in the request");
    }
    let mut messages = conversation.messages;

    let mut system = vec![SystemContentBlock::Text(SYSTEM_PROMPT.to_string())];
    system.extend(conversation.system.into_iter().map(SystemContentBlock::Text));
    let tool_config = state.tools.tool_config()?;

    for _ in 0..MAX_MODEL_CALLS {
        let output = state
            .bedrock
            .converse_stream()
            .model_id(&state.model_id)
            .set_system(Some(system.clone()))
            .set_messages(Some(messages.clone()))
            .tool_config(tool_config.clone())
            .inference_config(InferenceConfiguration::builder().max_tokens(MAX_TOKENS).build())
            .send()
            .await
            .map_err(|e| anyhow!("ConverseStream failed: {}", DisplayErrorContext(e)))?;

        let turn = stream_turn(output.stream, emitter).await?;
        messages.push(
            Message::builder()
                .role(ConversationRole::Assistant)
                .set_content(Some(turn.content))
                .build()?,
        );

        if turn.stop_reason != Some(StopReason::ToolUse) || turn.tool_uses.is_empty() {
            return Ok(());
        }

        let mut results = Vec::with_capacity(turn.tool_uses.len());
        for tool_use in turn.tool_uses {
            results.push(execute_tool(state, tool_use, emitter).await?);
        }
        messages.push(
            Message::builder()
                .role(ConversationRole::User)
                .set_content(Some(results))
                .build()?,
        );
    }
    bail!("model called tools more than {MAX_MODEL_CALLS} times")
}

struct PendingToolUse {
    id: String,
    name: String,
    input: Value,
}

struct Turn {
    content: Vec<ContentBlock>,
    tool_uses: Vec<PendingToolUse>,
    stop_reason: Option<StopReason>,
}

/// 開いているコンテンツブロック。Claude の ConverseStream では、テキストブロックには
/// `contentBlockStart` が来ないため、最初のテキスト差分で開く。
enum OpenBlock {
    Text { message_id: String, text: String },
    ToolUse { id: String, name: String, input: String },
}

/// モデル 1 回分のストリームを読み、テキストとツール呼び出しを AG-UI イベントとして送る。
async fn stream_turn(
    mut stream: aws_sdk_bedrockruntime::primitives::event_stream::EventReceiver<
        ConverseStreamOutput,
        aws_sdk_bedrockruntime::types::error::ConverseStreamOutputError,
    >,
    emitter: &Emitter,
) -> Result<Turn> {
    let mut turn = Turn {
        content: Vec::new(),
        tool_uses: Vec::new(),
        stop_reason: None,
    };
    let mut open: Option<OpenBlock> = None;

    while let Some(event) = stream
        .recv()
        .await
        .map_err(|e| anyhow!("ConverseStream event error: {}", DisplayErrorContext(e)))?
    {
        match event {
            ConverseStreamOutput::ContentBlockStart(start) => {
                if let Some(ContentBlockStart::ToolUse(tool)) = start.start {
                    emitter
                        .emit(Event::ToolCallStart {
                            tool_call_id: tool.tool_use_id.clone(),
                            tool_call_name: tool.name.clone(),
                        })
                        .await?;
                    open = Some(OpenBlock::ToolUse {
                        id: tool.tool_use_id,
                        name: tool.name,
                        input: String::new(),
                    });
                }
            }
            ConverseStreamOutput::ContentBlockDelta(delta) => match (delta.delta, &mut open) {
                (Some(ContentBlockDelta::Text(chunk)), _) if chunk.is_empty() => {}
                (Some(ContentBlockDelta::ToolUse(chunk)), _) if chunk.input.is_empty() => {}
                (Some(ContentBlockDelta::Text(chunk)), Some(OpenBlock::Text { message_id, text })) => {
                    text.push_str(&chunk);
                    emitter
                        .emit(Event::TextMessageContent {
                            message_id: message_id.clone(),
                            delta: chunk,
                        })
                        .await?;
                }
                (Some(ContentBlockDelta::Text(chunk)), None) => {
                    let message_id = Uuid::new_v4().to_string();
                    emitter
                        .emit(Event::TextMessageStart {
                            message_id: message_id.clone(),
                            role: "assistant",
                        })
                        .await?;
                    emitter
                        .emit(Event::TextMessageContent {
                            message_id: message_id.clone(),
                            delta: chunk.clone(),
                        })
                        .await?;
                    open = Some(OpenBlock::Text { message_id, text: chunk });
                }
                (Some(ContentBlockDelta::ToolUse(chunk)), Some(OpenBlock::ToolUse { id, input, .. })) => {
                    input.push_str(&chunk.input);
                    emitter
                        .emit(Event::ToolCallArgs {
                            tool_call_id: id.clone(),
                            delta: chunk.input,
                        })
                        .await?;
                }
                // reasoning などは扱わない。
                _ => {}
            },
            ConverseStreamOutput::ContentBlockStop(_) => match open.take() {
                Some(OpenBlock::Text { message_id, text }) => {
                    emitter.emit(Event::TextMessageEnd { message_id }).await?;
                    if !text.trim().is_empty() {
                        turn.content.push(ContentBlock::Text(text));
                    }
                }
                Some(OpenBlock::ToolUse { id, name, input }) => {
                    emitter.emit(Event::ToolCallEnd { tool_call_id: id.clone() }).await?;
                    let input: Value = if input.trim().is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(&input).with_context(|| format!("invalid tool input for {name}"))?
                    };
                    turn.content.push(ContentBlock::ToolUse(
                        ToolUseBlock::builder()
                            .tool_use_id(&id)
                            .name(&name)
                            .input(json_to_document(input.clone()))
                            .build()?,
                    ));
                    turn.tool_uses.push(PendingToolUse { id, name, input });
                }
                None => {}
            },
            ConverseStreamOutput::MessageStop(stop) => turn.stop_reason = Some(stop.stop_reason),
            _ => {}
        }
    }
    Ok(turn)
}

/// ツールを実行して `TOOL_CALL_RESULT` を送り、Converse に返す toolResult を作る。
/// ツールのエラーはモデルに伝えて続行させるため、ここでは `Err` にしない。
async fn execute_tool(state: &AppState, tool_use: PendingToolUse, emitter: &Emitter) -> Result<ContentBlock> {
    let (content, status) = match state.tools.call(&tool_use.name, tool_use.input).await {
        Ok(value) => (value.to_string(), ToolResultStatus::Success),
        Err(message) => {
            tracing::warn!(tool = tool_use.name, message, "tool failed");
            (json!({ "error": message }).to_string(), ToolResultStatus::Error)
        }
    };
    emitter
        .emit(Event::ToolCallResult {
            message_id: Uuid::new_v4().to_string(),
            tool_call_id: tool_use.id.clone(),
            content: content.clone(),
            role: "tool",
        })
        .await?;
    Ok(ContentBlock::ToolResult(
        ToolResultBlock::builder()
            .tool_use_id(tool_use.id)
            .content(ToolResultContentBlock::Text(content))
            .status(status)
            .build()?,
    ))
}
