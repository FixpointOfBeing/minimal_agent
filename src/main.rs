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
pub mod prompt;

pub mod error;
pub use error::AgentError;

#[tokio::main]
async fn main() -> Result<(), AgentError> {
    repl().await
}
