//! エージェントが使うツール。
//!
//! ツールの結果は JSON で返し、そのままの JSON 文字列を AG-UI の
//! `TOOL_CALL_RESULT.content` としてフロントエンドに送る。フロントエンドは
//! `useRenderTool` でこの JSON をグラフにする（`frontend/app/page.tsx`）。

use anyhow::Result;
use aws_sdk_bedrockruntime::types::{Tool as ConverseTool, ToolConfiguration, ToolInputSchema, ToolSpecification};
use chrono::{Days, Utc};
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::convert::json_to_document;

pub trait Tool: Send + Sync {
    fn name(&self) -> &'static str;
    fn description(&self) -> &'static str;
    /// 引数の JSON Schema。
    fn input_schema(&self) -> Value;
    /// 失敗した場合はモデルとユーザーに見せるエラーメッセージを返す。
    fn call(&self, args: Value) -> BoxFuture<'_, Result<Value, String>>;
}

pub struct ToolRegistry {
    tools: Vec<Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new(tools: Vec<Box<dyn Tool>>) -> Self {
        Self { tools }
    }

    pub fn tool_config(&self) -> Result<ToolConfiguration> {
        let tools = self
            .tools
            .iter()
            .map(|tool| {
                Ok(ConverseTool::ToolSpec(
                    ToolSpecification::builder()
                        .name(tool.name())
                        .description(tool.description())
                        .input_schema(ToolInputSchema::Json(json_to_document(tool.input_schema())))
                        .build()?,
                ))
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(ToolConfiguration::builder().set_tools(Some(tools)).build()?)
    }

    pub async fn call(&self, name: &str, args: Value) -> Result<Value, String> {
        match self.tools.iter().find(|tool| tool.name() == name) {
            Some(tool) => tool.call(args).await,
            None => Err(format!("unknown tool: {name}")),
        }
    }
}

pub fn default_registry() -> ToolRegistry {
    ToolRegistry::new(vec![Box::new(SampleMetrics), Box::new(CurrentTime)])
}

/// 決まった規則で生成したダミーのメトリクスを日別に返す。グラフ表示の確認用。
pub struct SampleMetrics;

#[derive(Deserialize)]
struct SampleMetricsArgs {
    metric: String,
    #[serde(default = "default_days")]
    days: u32,
}

fn default_days() -> u32 {
    7
}

const MAX_DAYS: u32 = 30;

impl Tool for SampleMetrics {
    fn name(&self) -> &'static str {
        "get_sample_metrics"
    }

    fn description(&self) -> &'static str {
        "デモ用のサンプルメトリクス（ダミー値）を日別に取得する。結果は画面にグラフとして表示される。"
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "metric": {
                    "type": "string",
                    "enum": ["cpu", "latency", "requests"],
                    "description": "cpu: CPU 使用率（%）、latency: p95 レイテンシ（ms）、requests: リクエスト数"
                },
                "days": {
                    "type": "integer", "minimum": 1, "maximum": MAX_DAYS,
                    "description": "今日から遡る日数。省略時は 7"
                }
            },
            "required": ["metric"]
        })
    }

    fn call(&self, args: Value) -> BoxFuture<'_, Result<Value, String>> {
        Box::pin(async move {
            let args: SampleMetricsArgs = serde_json::from_value(args).map_err(|e| format!("invalid arguments: {e}"))?;
            if !(1..=MAX_DAYS).contains(&args.days) {
                return Err(format!("days must be between 1 and {MAX_DAYS}"));
            }
            let (base, amplitude, unit) = match args.metric.as_str() {
                "cpu" => (45.0, 20.0, "%"),
                "latency" => (120.0, 40.0, "ms"),
                "requests" => (12000.0, 4000.0, "count"),
                other => return Err(format!("unknown metric: {other}")),
            };
            let today = Utc::now().date_naive();
            let points: Vec<Value> = (0..args.days)
                .rev()
                .map(|offset| {
                    let date = today - Days::new(offset.into());
                    let x = f64::from(args.days - offset);
                    let value = base + amplitude * (x * 0.9).sin() + amplitude * 0.3 * (x * 2.3).cos();
                    json!({ "date": date.to_string(), "value": (value * 10.0).round() / 10.0 })
                })
                .collect();
            Ok(json!({ "metric": args.metric, "unit": unit, "points": points }))
        })
    }
}

pub struct CurrentTime;

impl Tool for CurrentTime {
    fn name(&self) -> &'static str {
        "get_current_time"
    }

    fn description(&self) -> &'static str {
        "現在の日時（UTC）を取得する。"
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "properties": {} })
    }

    fn call(&self, _args: Value) -> BoxFuture<'_, Result<Value, String>> {
        Box::pin(async move { Ok(json!({ "utc": Utc::now().to_rfc3339() })) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sample_metrics_returns_requested_number_of_points() {
        let result = default_registry()
            .call("get_sample_metrics", json!({"metric": "cpu", "days": 3}))
            .await
            .unwrap();
        assert_eq!(result["unit"], "%");
        assert_eq!(result["points"].as_array().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn sample_metrics_rejects_out_of_range_days() {
        let err = default_registry()
            .call("get_sample_metrics", json!({"metric": "cpu", "days": 0}))
            .await
            .unwrap_err();
        assert!(err.contains("days"));
    }

    #[test]
    fn builds_tool_config() {
        assert_eq!(default_registry().tool_config().unwrap().tools().len(), 2);
    }
}
