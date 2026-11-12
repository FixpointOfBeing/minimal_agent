use std::collections::HashMap;

pub mod read;
pub use read::ReadTool;
pub mod write;
use serde_json::Value;
pub use write::WriteTool;

use crate::Context;

pub struct ToolRegistry {
    tools: HashMap<String, Tool>,
}

impl ToolRegistry {
    fn tools() -> Vec<Tool> {
        vec![Tool::Read(ReadTool {}), Tool::Write(WriteTool {})]
    }

    pub fn new() -> Self {
        let mut tools_map = HashMap::new();
        for tool in Self::tools() {
            tools_map.insert(tool.spec().name.to_string(), tool);
        }
        ToolRegistry { tools: tools_map }
    }

    pub fn tool_specs(&self) -> Vec<ToolSpec> {
        self.tools.values().map(|tool| tool.spec()).collect()
    }

    pub fn get(&self, name: &str) -> Option<&Tool> {
        self.tools.get(name)
    }
}

pub enum Tool {
    Read(ReadTool),
    Write(WriteTool),
}
#[derive(Clone, Debug)]
pub struct Call {
    pub call_id: String,
    pub name: String,
    pub arguments: Value,
}

impl Tool {
    pub fn spec(&self) -> ToolSpec {
        match self {
            Tool::Read(read_tool) => read_tool.spec(),
            Tool::Write(write_tool) => write_tool.spec(),
        }
    }

    pub async fn execute(&self, ctx: &Context, call: Call) -> ToolOutput {
        match self {
            Tool::Read(read_tool) => read_tool.execute(ctx, call).await,
            Tool::Write(write_tool) => write_tool.execute(ctx, call).await,
        }
    }
}

pub struct ToolSpec {
    pub name: &'static str,

    pub parameters: Value,

    /// One-line summary shown in the system prompt.
    pub snippet: &'static str,

    /// Longer description shown in the tool definition sent to the provider.
    pub description: &'static str,

    /// Guideline bullets rendered into the system prompt's `<rules>` block.
    pub guidelines: &'static [&'static str],
}

pub enum ToolOutput {
    Ok { call: Call, output: Value },
    Err { call: Call, error: String },
}

impl ToolOutput {
    pub fn ok(call: &Call, output: Value) -> ToolOutput {
        ToolOutput::Ok { call: call.clone(), output }
    }

    pub fn err(call: &Call, error: impl Into<String>) -> ToolOutput {
        ToolOutput::Err { call: call.clone(), error: error.into() }
    }
}
