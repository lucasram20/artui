//! Per-provider serialization of `ToolSpec` and `ToolChoice` into JSON request bodies.

use serde_json::{json, Value};

use super::{ToolChoice, ToolSpec};

/// Serialize tools for OpenAI Chat Completions API.
/// Shape: `[{type: "function", function: {name, description, parameters}}]`
pub fn to_openai_chat(tools: &[ToolSpec]) -> Value {
    tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters,
                }
            })
        })
        .collect()
}

/// Serialize tools for OpenAI Responses API (flat shape).
/// Shape: `[{type: "function", name, description, parameters}]`
pub fn to_openai_responses(tools: &[ToolSpec]) -> Value {
    tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.parameters,
            })
        })
        .collect()
}

/// Serialize tools for Anthropic Messages API.
/// Shape: `[{name, description, input_schema}]`
pub fn to_anthropic_messages(tools: &[ToolSpec]) -> Value {
    tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.parameters,
            })
        })
        .collect()
}

/// Serialize tools for Ollama 0.4+ API.
/// Shape: `[{type: "function", function: {name, description, parameters}}]`
/// (Same as OpenAI Chat Completions.)
pub fn to_ollama(tools: &[ToolSpec]) -> Value {
    to_openai_chat(tools)
}

/// Serialize `ToolChoice` for OpenAI Chat Completions / Responses API.
pub fn tool_choice_to_openai(choice: &ToolChoice) -> Value {
    match choice {
        ToolChoice::Auto => json!("auto"),
        ToolChoice::Required => json!("required"),
        ToolChoice::None => json!("none"),
        ToolChoice::Specific(name) => json!({"type": "function", "function": {"name": name}}),
    }
}

/// Serialize `ToolChoice` for Anthropic Messages API.
pub fn tool_choice_to_anthropic(choice: &ToolChoice) -> Value {
    match choice {
        ToolChoice::Auto => json!({"type": "auto"}),
        ToolChoice::Required => json!({"type": "any"}),
        ToolChoice::None => json!({"type": "none"}),
        ToolChoice::Specific(name) => json!({"type": "tool", "name": name}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tool() -> ToolSpec {
        ToolSpec {
            name: "read_file".to_owned(),
            description: "Read a file from the workspace".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Relative path to the file"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    #[test]
    fn openai_chat_shape() {
        let tools = vec![sample_tool()];
        let serialized = to_openai_chat(&tools);
        let arr = serialized.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        let tool = &arr[0];
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["function"]["name"], "read_file");
        assert_eq!(
            tool["function"]["description"],
            "Read a file from the workspace"
        );
        assert!(tool["function"]["parameters"]["properties"]["path"].is_object());
    }

    #[test]
    fn openai_responses_shape() {
        let tools = vec![sample_tool()];
        let serialized = to_openai_responses(&tools);
        let arr = serialized.as_array().unwrap();
        let tool = &arr[0];
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["name"], "read_file");
        assert_eq!(tool["description"], "Read a file from the workspace");
        assert!(tool["parameters"]["properties"]["path"].is_object());
    }

    #[test]
    fn anthropic_messages_shape() {
        let tools = vec![sample_tool()];
        let serialized = to_anthropic_messages(&tools);
        let arr = serialized.as_array().unwrap();
        let tool = &arr[0];
        assert_eq!(tool["name"], "read_file");
        assert_eq!(tool["description"], "Read a file from the workspace");
        assert!(tool["input_schema"]["properties"]["path"].is_object());
        // No "type" field at top level for Anthropic
        assert!(tool.get("type").is_none());
    }

    #[test]
    fn ollama_matches_openai_chat() {
        let tools = vec![sample_tool()];
        assert_eq!(to_ollama(&tools), to_openai_chat(&tools));
    }

    #[test]
    fn tool_choice_serialization() {
        assert_eq!(tool_choice_to_openai(&ToolChoice::Auto), json!("auto"));
        assert_eq!(
            tool_choice_to_openai(&ToolChoice::Required),
            json!("required")
        );
        assert_eq!(tool_choice_to_openai(&ToolChoice::None), json!("none"));
        assert_eq!(
            tool_choice_to_openai(&ToolChoice::Specific("read_file".to_owned())),
            json!({"type": "function", "function": {"name": "read_file"}})
        );
    }

    #[test]
    fn tool_choice_anthropic_serialization() {
        assert_eq!(
            tool_choice_to_anthropic(&ToolChoice::Auto),
            json!({"type": "auto"})
        );
        assert_eq!(
            tool_choice_to_anthropic(&ToolChoice::Required),
            json!({"type": "any"})
        );
        assert_eq!(
            tool_choice_to_anthropic(&ToolChoice::Specific("read_file".to_owned())),
            json!({"type": "tool", "name": "read_file"})
        );
    }

    #[test]
    fn round_trip_tool_spec_serde() {
        let tool = sample_tool();
        let serialized = serde_json::to_value(&tool).unwrap();
        assert_eq!(serialized["name"], "read_file");
        assert_eq!(serialized["description"], "Read a file from the workspace");
        assert!(serialized["parameters"]["required"]
            .as_array()
            .unwrap()
            .contains(&json!("path")));
    }
}
