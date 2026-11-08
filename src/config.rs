use crate::{Effort, LlmError};
use std::env;

#[derive(Clone)]
pub struct Config {
    pub api_key: String,
    pub endpoint: String,
    pub model: String,
    pub reasoning: Effort,
    pub system_prompt: String,
}

impl Config {
    pub fn new() -> Result<Config, LlmError> {
        let api_key = env::var("MINIMAL_AGENT_API_KEY")
            .map_err(|e| LlmError(format!("MINIMAL_AGENT_API_KEY: {e}")))?;

        let endpoint = env::var("MINIMAL_AGENT_BASE_URL")
            .map_err(|e| LlmError(format!("MINIMAL_AGENT_BASE_URL: {e}")))?;

        let model = env::var("MINIMAL_AGENT_MODEL")
            .map_err(|e| LlmError(format!("MINIMAL_AGENT_MODEL: {e}")))?;

        let reasoning = match env::var("MINIMAL_AGENT_REASONING")
            .unwrap_or_else(|_| "none".into())
            .to_ascii_lowercase()
            .as_str()
        {
            "none" => Effort::None,
            "low" => Effort::Low,
            "high" => Effort::High,
            "max" => Effort::Max,
            other => {
                return Err(LlmError(format!(
                    "MINIMAL_AGENT_REASONING 取值非法: {other:?}，可选 none/low/high/max"
                )));
            },
        };

        let system_prompt = "you are a helpful assistant".to_string();
        Ok(Config { api_key, endpoint, model, reasoning, system_prompt })
    }
}
