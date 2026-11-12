use serde_json::{Value, json};

use crate::Context;
use crate::tools::{Call, ToolOutput, ToolSpec};
pub struct WriteTool {}

impl WriteTool {
    pub fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "write",
            snippet: "Create or overwrite files",
            description: "Write content to a file. Creates the file if it doesn't exist, overwrites if it does. Automatically creates parent directories.",
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to write (relative or absolute)"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file"
                    }
                },
                "required": ["path", "content"],
                "additionalProperties": false
            }),
            guidelines: &["Use write only for new files or complete rewrites"],
        }
    }

    pub async fn execute(&self, ctx: &Context, call: Call) -> ToolOutput {
        let args = call.arguments.clone();
        let Some(path) = args.get("path").and_then(Value::as_str) else {
            return ToolOutput::err(&call, "write requires a string `path` argument");
        };
        let Some(content) = args.get("content").and_then(Value::as_str) else {
            return ToolOutput::err(&call, "write requires a string `content` argument");
        };

        let absolute = match ctx.resolve_for_mutation(path) {
            Ok(path) => path,
            Err(error) => return ToolOutput::err(&call, error),
        };

        if let Some(parent) = absolute.parent()
            && let Err(error) = tokio::fs::create_dir_all(parent).await
        {
            return ToolOutput::err(
                &call,
                format!("write: cannot create parent directory for {path}: {error}"),
            );
        }

        match tokio::fs::write(&absolute, content).await {
            Ok(()) => ToolOutput::ok(
                &call,
                json!({
                    "result": format!("Successfully wrote {} byte(s) to {path}.", content.len())
                }),
            ),
            Err(error) => ToolOutput::err(&call, format!("write: cannot write {path}: {error}")),
        }
    }
}
