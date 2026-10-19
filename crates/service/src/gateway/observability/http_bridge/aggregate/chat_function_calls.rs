use std::collections::BTreeMap;

use serde_json::{json, Value};

#[derive(Debug, Clone, Default)]
struct FunctionCall {
    id: String,
    name: String,
    arguments: String,
}

/// Native Chat indices identify tools within a choice, independently of the
/// order in which their interleaved deltas arrive.
#[derive(Debug, Clone, Default)]
pub(super) struct ChatFunctionCalls(BTreeMap<u64, FunctionCall>);

impl ChatFunctionCalls {
    pub(super) fn merge_delta(&mut self, delta: &Value) {
        let Some(tools) = delta.get("tool_calls").and_then(Value::as_array) else {
            return;
        };
        for tool in tools {
            let Some(index) = tool.get("index").and_then(Value::as_u64) else {
                continue;
            };
            let kind = tool.get("type").and_then(Value::as_str);
            if kind.is_some_and(|kind| kind != "function") {
                continue;
            }
            let function = tool.get("function").and_then(Value::as_object);
            if kind.is_none() && function.is_none() && !self.0.contains_key(&index) {
                continue;
            }
            let target = self.0.entry(index).or_default();
            if let Some(id) = tool.get("id").and_then(Value::as_str) {
                target.id.push_str(id);
            }
            if let Some(function) = function {
                if let Some(name) = function.get("name").and_then(Value::as_str) {
                    target.name.push_str(name);
                }
                if let Some(arguments) = function.get("arguments").and_then(Value::as_str) {
                    // Native Chat sends append-only fragments, not snapshots.
                    // Identical consecutive fragments can be meaningful JSON.
                    target.arguments.push_str(arguments);
                }
            }
        }
    }

    pub(super) fn snapshot(&self) -> Vec<Value> {
        self.0
            .values()
            .map(|tool| {
                let mut value = json!({
                    "type": "function",
                    "function": { "name": tool.name, "arguments": tool.arguments }
                });
                if !tool.id.is_empty() {
                    value["id"] = json!(tool.id);
                }
                value
            })
            .collect()
    }
}
