//! Amazon Bedrock AgentCore Runtime 用の Rust エージェント。
//!
//! Runtime のコンテナ契約（`0.0.0.0:8080`、`GET /ping`、`POST /invocations`）を満たし、
//! `/invocations` では AG-UI プロトコルのイベントを SSE で返す
//! （Runtime 側は `protocolConfiguration: AGUI`）。

mod agent_loop;
mod agui;
mod convert;
mod tools;

use std::convert::Infallible;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::sse::{Event as SseEvent, KeepAlive, Sse};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures::Stream;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::ReceiverStream;
use tracing_subscriber::EnvFilter;

use crate::agui::RunAgentInput;
use crate::tools::ToolRegistry;

const DEFAULT_MODEL_ID: &str = "us.anthropic.claude-haiku-4-5-20251001-v1:0";
const SESSION_ID_HEADER: &str = "x-amzn-bedrock-agentcore-runtime-session-id";
/// 長いツール実行やモデルの思考中に接続が無通信で切られないよう、SSE のコメントを送る間隔。
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);

#[derive(Clone)]
pub struct AppState {
    bedrock: aws_sdk_bedrockruntime::Client,
    model_id: String,
    tools: Arc<ToolRegistry>,
    /// 処理中の run の数。`/ping` で `HealthyBusy` を返すかどうかに使う。
    active_runs: Arc<AtomicUsize>,
}

/// run の終了時（接続が切れて途中で止まった場合も含む）に `active_runs` を戻す。
struct ActiveRunGuard(Arc<AtomicUsize>);

impl ActiveRunGuard {
    fn new(counter: Arc<AtomicUsize>) -> Self {
        counter.fetch_add(1, Ordering::SeqCst);
        Self(counter)
    }
}

impl Drop for ActiveRunGuard {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();

    let aws_config = aws_config::load_from_env().await;
    let state = AppState {
        bedrock: aws_sdk_bedrockruntime::Client::new(&aws_config),
        model_id: std::env::var("BEDROCK_MODEL_ID").unwrap_or_else(|_| DEFAULT_MODEL_ID.to_string()),
        tools: Arc::new(tools::default_registry()),
        active_runs: Arc::new(AtomicUsize::new(0)),
    };
    tracing::info!(model_id = state.model_id, region = ?aws_config.region(), "starting agent");

    let app = Router::new()
        .route("/ping", get(ping))
        .route("/invocations", post(invocations))
        .with_state(state);

    let port = std::env::var("PORT").unwrap_or_else(|_| "8080".to_string());
    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{port}")).await?;
    tracing::info!(addr = %listener.local_addr()?, "listening");
    axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()).await?;
    Ok(())
}

async fn ping(State(state): State<AppState>) -> Json<Value> {
    let status = if state.active_runs.load(Ordering::SeqCst) > 0 {
        "HealthyBusy"
    } else {
        "Healthy"
    };
    Json(json!({ "status": status }))
}

async fn invocations(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<RunAgentInput>,
) -> Sse<impl Stream<Item = Result<SseEvent, Infallible>>> {
    let session_id = headers
        .get(SESSION_ID_HEADER)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("-");
    tracing::info!(
        session_id,
        thread_id = input.thread_id,
        run_id = input.run_id,
        messages = input.messages.len(),
        "invocation"
    );

    let (tx, rx) = mpsc::channel(64);
    let guard = ActiveRunGuard::new(state.active_runs.clone());
    tokio::spawn(async move {
        let _guard = guard;
        agent_loop::run(state, input, tx).await;
    });

    let stream = ReceiverStream::new(rx).map(|event| {
        Ok(SseEvent::default()
            .json_data(&event)
            .expect("AG-UI events always serialize to JSON"))
    });
    Sse::new(stream).keep_alive(KeepAlive::new().interval(HEARTBEAT_INTERVAL).text("ping"))
}

async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let terminate = async {
        if let Ok(mut signal) = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()) {
            signal.recv().await;
        }
    };
    tokio::select! {
        () = ctrl_c => {},
        () = terminate => {},
    }
}
