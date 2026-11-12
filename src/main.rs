pub mod api;
pub use api::{
    Effort, Input, InputItem, InputMessageContent, LlmClient, OutputItem, OutputItemContent,
    Payload, Reasoning, ReasoningText, ResponseStatus, Role, StreamEventKind, StreamEventResponse,
    TextContentPart,
};
pub mod repl;
pub use repl::repl;

pub mod config;
pub use config::Config;

pub mod tools;
pub use tools::ToolRegistry;

pub mod agent_loop;
pub use agent_loop::run_turn;
pub mod prompt;
pub use prompt::build_system_prompt;

pub mod error;
pub use error::AgentError;

pub mod context;
pub use context::Context;

#[tokio::main]
async fn main() -> Result<(), AgentError> {
    repl().await
}
