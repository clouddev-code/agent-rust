//! AG-UI のメッセージ履歴を Bedrock Converse の形式に変換する。
//!
//! Converse には次の制約があるため、変換時に整える。
//! - 先頭は user で、user と assistant が交互に並ぶ（同じ role が続いたら 1 つにまとめる）。
//! - toolUse には、直後の user メッセージに対応する toolResult が必要（片方しかないものは捨てる）。
//! - 空のテキストブロックは受け付けない。

use std::collections::HashSet;

use anyhow::Result;
use aws_sdk_bedrockruntime::types::{
    ContentBlock, ConversationRole, Message, ToolResultBlock, ToolResultContentBlock, ToolUseBlock,
};
use aws_smithy_types::{Document, Number};
use serde_json::Value;

use crate::agui;

pub struct Conversation {
    /// system / developer メッセージの本文。システムプロンプトに追記する。
    pub system: Vec<String>,
    pub messages: Vec<Message>,
}

pub fn to_converse(history: &[agui::Message]) -> Result<Conversation> {
    let answered: HashSet<&str> = history
        .iter()
        .filter_map(|m| match m {
            agui::Message::Tool { tool_call_id, .. } => Some(tool_call_id.as_str()),
            _ => None,
        })
        .collect();
    let requested: HashSet<&str> = history
        .iter()
        .flat_map(|m| match m {
            agui::Message::Assistant { tool_calls, .. } => tool_calls.iter().map(|c| c.id.as_str()).collect(),
            _ => Vec::new(),
        })
        .collect();

    let mut system = Vec::new();
    let mut turns: Vec<(ConversationRole, Vec<ContentBlock>)> = Vec::new();

    for message in history {
        let (role, blocks) = match message {
            agui::Message::User { content } => {
                let text = agui::user_text(content);
                (ConversationRole::User, text_block(text).into_iter().collect())
            }
            agui::Message::Assistant { content, tool_calls } => {
                let mut blocks: Vec<ContentBlock> =
                    text_block(content.clone().unwrap_or_default()).into_iter().collect();
                for call in tool_calls.iter().filter(|c| answered.contains(c.id.as_str())) {
                    let input = if call.function.arguments.trim().is_empty() {
                        Value::Object(Default::default())
                    } else {
                        serde_json::from_str(&call.function.arguments)?
                    };
                    blocks.push(ContentBlock::ToolUse(
                        ToolUseBlock::builder()
                            .tool_use_id(&call.id)
                            .name(&call.function.name)
                            .input(json_to_document(input))
                            .build()?,
                    ));
                }
                (ConversationRole::Assistant, blocks)
            }
            agui::Message::Tool { content, tool_call_id } if requested.contains(tool_call_id.as_str()) => (
                ConversationRole::User,
                vec![ContentBlock::ToolResult(
                    ToolResultBlock::builder()
                        .tool_use_id(tool_call_id)
                        .content(ToolResultContentBlock::Text(content.clone()))
                        .build()?,
                )],
            ),
            agui::Message::System { content } | agui::Message::Developer { content } => {
                system.push(content.clone());
                continue;
            }
            agui::Message::Tool { .. } | agui::Message::Other => continue,
        };
        if blocks.is_empty() {
            continue;
        }
        match turns.last_mut() {
            Some((last_role, last_blocks)) if *last_role == role => last_blocks.extend(blocks),
            _ => turns.push((role, blocks)),
        }
    }

    // 先頭が assistant だと Converse がエラーにするため、最初の user まで読み飛ばす。
    let first_user = turns
        .iter()
        .position(|(role, _)| *role == ConversationRole::User)
        .unwrap_or(turns.len());

    let messages = turns
        .into_iter()
        .skip(first_user)
        .map(|(role, blocks)| Message::builder().role(role).set_content(Some(blocks)).build())
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Conversation { system, messages })
}

fn text_block(text: String) -> Option<ContentBlock> {
    (!text.trim().is_empty()).then_some(ContentBlock::Text(text))
}

pub fn json_to_document(value: Value) -> Document {
    match value {
        Value::Null => Document::Null,
        Value::Bool(b) => Document::Bool(b),
        Value::Number(n) => {
            if let Some(u) = n.as_u64() {
                Document::Number(Number::PosInt(u))
            } else if let Some(i) = n.as_i64() {
                Document::Number(Number::NegInt(i))
            } else {
                Document::Number(Number::Float(n.as_f64().unwrap_or_default()))
            }
        }
        Value::String(s) => Document::String(s),
        Value::Array(items) => Document::Array(items.into_iter().map(json_to_document).collect()),
        Value::Object(map) => Document::Object(map.into_iter().map(|(k, v)| (k, json_to_document(v))).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn history(value: Value) -> Vec<agui::Message> {
        serde_json::from_value(value).unwrap()
    }

    fn roles(conv: &Conversation) -> Vec<ConversationRole> {
        conv.messages.iter().map(|m| m.role().clone()).collect()
    }

    #[test]
    fn merges_consecutive_roles_and_pairs_tool_calls() {
        let conv = to_converse(&history(json!([
            {"id": "1", "role": "user", "content": "メトリクスを見せて"},
            {"id": "2", "role": "assistant", "content": "取得します"},
            {"id": "3", "role": "assistant", "toolCalls": [
                {"id": "t1", "type": "function", "function": {"name": "get_sample_metrics", "arguments": "{\"days\":3}"}}
            ]},
            {"id": "4", "role": "tool", "toolCallId": "t1", "content": "[]"},
            {"id": "5", "role": "assistant", "content": "どうぞ"},
            {"id": "6", "role": "user", "content": "ありがとう"}
        ])))
        .unwrap();

        use ConversationRole::{Assistant, User};
        assert_eq!(roles(&conv), vec![User, Assistant, User, Assistant, User]);
        let assistant = conv.messages[1].content();
        assert_eq!(assistant.len(), 2);
        assert!(assistant[0].is_text());
        assert_eq!(assistant[1].as_tool_use().unwrap().tool_use_id(), "t1");
        assert!(conv.messages[2].content()[0].is_tool_result());
    }

    #[test]
    fn drops_unanswered_tool_calls_and_orphan_results() {
        let conv = to_converse(&history(json!([
            {"id": "1", "role": "user", "content": "hi"},
            {"id": "2", "role": "assistant", "content": "", "toolCalls": [
                {"id": "t1", "type": "function", "function": {"name": "get_current_time", "arguments": ""}}
            ]},
            {"id": "3", "role": "tool", "toolCallId": "unknown", "content": "{}"},
            {"id": "4", "role": "user", "content": "again"}
        ])))
        .unwrap();

        assert_eq!(roles(&conv), vec![ConversationRole::User]);
        assert_eq!(conv.messages[0].content().len(), 2);
    }

    #[test]
    fn collects_system_messages_and_skips_leading_assistant() {
        let conv = to_converse(&history(json!([
            {"id": "0", "role": "system", "content": "丁寧に"},
            {"id": "1", "role": "assistant", "content": "こんにちは"},
            {"id": "2", "role": "user", "content": "hi"}
        ])))
        .unwrap();

        assert_eq!(conv.system, vec!["丁寧に".to_string()]);
        assert_eq!(roles(&conv), vec![ConversationRole::User]);
    }

    #[test]
    fn converts_json_numbers_to_matching_document_numbers() {
        let doc = json_to_document(json!({"a": 1, "b": -2, "c": 1.5}));
        let Document::Object(map) = doc else { panic!() };
        assert_eq!(map["a"], Document::Number(Number::PosInt(1)));
        assert_eq!(map["b"], Document::Number(Number::NegInt(-2)));
        assert_eq!(map["c"], Document::Number(Number::Float(1.5)));
    }
}
