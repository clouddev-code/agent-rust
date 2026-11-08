//! AG-UI プロトコルの入出力型。
//!
//! フィールド名は `@ag-ui/client` 0.0.57 と、参考リポジトリの Python 実装
//! （`ag_ui.core` の `RunAgentInput` / 各 Event）に合わせて camelCase にする。
//! フロントエンドが送ってくる `tools` / `context` / `state` / `forwardedProps` は
//! このエージェントでは使わないため、デシリアライズ時に読み捨てる。

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunAgentInput {
    pub thread_id: String,
    pub run_id: String,
    #[serde(default)]
    pub messages: Vec<Message>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    User {
        /// 文字列、または `[{type:"text", text}, ...]` の配列。
        #[serde(default)]
        content: Value,
    },
    Assistant {
        #[serde(default)]
        content: Option<String>,
        #[serde(default, rename = "toolCalls")]
        tool_calls: Vec<ToolCall>,
    },
    Tool {
        content: String,
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
    },
    System {
        content: String,
    },
    Developer {
        content: String,
    },
    /// activity / reasoning など、モデルへの入力にしないメッセージ。
    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub function: FunctionCall,
}

#[derive(Debug, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    /// JSON 文字列。
    #[serde(default)]
    pub arguments: String,
}

/// ユーザーメッセージの `content` からテキストを取り出す。
pub fn user_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter(|part| part.get("type").and_then(Value::as_str) == Some("text"))
            .filter_map(|part| part.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// フロントエンドへ SSE（`data: <JSON>`）で送るイベント。
#[derive(Debug, Serialize)]
#[serde(
    tag = "type",
    rename_all = "SCREAMING_SNAKE_CASE",
    rename_all_fields = "camelCase"
)]
pub enum Event {
    RunStarted {
        thread_id: String,
        run_id: String,
    },
    RunFinished {
        thread_id: String,
        run_id: String,
    },
    RunError {
        message: String,
    },
    TextMessageStart {
        message_id: String,
        role: &'static str,
    },
    TextMessageContent {
        message_id: String,
        delta: String,
    },
    TextMessageEnd {
        message_id: String,
    },
    ToolCallStart {
        tool_call_id: String,
        tool_call_name: String,
    },
    ToolCallArgs {
        tool_call_id: String,
        delta: String,
    },
    ToolCallEnd {
        tool_call_id: String,
    },
    ToolCallResult {
        message_id: String,
        tool_call_id: String,
        content: String,
        role: &'static str,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn events_serialize_with_agui_field_names() {
        let event = Event::ToolCallResult {
            message_id: "m1".into(),
            tool_call_id: "t1".into(),
            content: "[]".into(),
            role: "tool",
        };
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            json!({"type":"TOOL_CALL_RESULT","messageId":"m1","toolCallId":"t1","content":"[]","role":"tool"})
        );

        let event = Event::RunStarted {
            thread_id: "th".into(),
            run_id: "r".into(),
        };
        assert_eq!(
            serde_json::to_value(event).unwrap(),
            json!({"type":"RUN_STARTED","threadId":"th","runId":"r"})
        );
    }

    #[test]
    fn deserializes_run_agent_input_and_ignores_unused_fields() {
        let input: RunAgentInput = serde_json::from_value(json!({
            "threadId": "th", "runId": "r",
            "messages": [
                {"id": "1", "role": "user", "content": "hi"},
                {"id": "2", "role": "assistant", "content": "",
                 "toolCalls": [{"id": "t1", "type": "function",
                                "function": {"name": "get_current_time", "arguments": "{}"}}]},
                {"id": "3", "role": "tool", "content": "{}", "toolCallId": "t1"},
                {"id": "4", "role": "activity", "content": {}}
            ],
            "tools": [], "context": [], "state": {}, "forwardedProps": {}
        }))
        .unwrap();
        assert_eq!(input.messages.len(), 4);
        assert!(matches!(input.messages[3], Message::Other));
    }

    #[test]
    fn extracts_text_from_multipart_user_content() {
        let content = json!([{"type": "text", "text": "a"}, {"type": "binary"}, {"type": "text", "text": "b"}]);
        assert_eq!(user_text(&content), "a\nb");
    }
}
