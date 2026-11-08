
pub mod api;
pub use api::{StreamEventKind, LlmClient, LlmError, InputItem, Role, Input, Payload, Effort, Reasoning, InputMessageContent};
pub mod repl;
pub use repl::repl;

pub mod config;
pub use config::Config;
#[tokio::main]
async fn main() -> Result<(), LlmError> {
    let config = Config::new()?;
    repl(config).await
}
