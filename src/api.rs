use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt, TryFutureExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;
use std::time::Duration;

use crate::AgentError;
use crate::Config;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Payload {
    pub model: String,

    pub instructions: String,

    pub input: Input,

    pub reasoning: Reasoning,

    pub stream: bool,

    pub tools: Vec<ToolDefinition>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolDefinition {
    Function { name: String, description: String, parameters: Value },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Reasoning {
    pub effort: Effort,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum Input {
    Text(String),

    Items(Vec<InputItem>),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputItem {
    Message { role: Role, content: InputMessageContent },

    Reasoning { content: Vec<ReasoningText> },

    FunctionCallOutput { call_id: String, output: String },

    FunctionCall { call_id: String, name: String, arguments: String },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(untagged)]
pub enum InputMessageContent {
    Text(String),

    ContentParts(Vec<TextContentPart>),
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TextContentPart {
    InputText { text: String },

    OutputText { text: String },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReasoningText {
    ReasoningText { text: String },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,

    Assistant,

    System,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    None,

    Low,

    High,

    Max,
}

pub struct LlmClient {
    http: reqwest::Client,

    endpoint: String,

    api_key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StreamEventKind {
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta,

    #[serde(rename = "response.reasoning_text.delta")]
    ReasoningTextDelta,

    #[serde(rename = "response.output_item.done")]
    OutputItemDone,

    #[serde(rename = "response.reasoning_text.done")]
    ReasoningTextDone,

    #[serde(rename = "response.output_text.done")]
    OutputTextDone,

    #[serde(untagged)]
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseStatus {
    InProgress,

    Completed,

    Incomplete,

    Failed,

    #[serde(untagged)]
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutputItemContent {
    OutputText { text: String },

    ReasoningText { text: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum OutputItem {
    Message { id: String, content: Vec<OutputItemContent> },

    Reasoning { id: String, content: Vec<OutputItemContent> },

    FunctionCall { call_id: String, name: String, arguments: String },
}
impl From<OutputItem> for InputItem {
    fn from(value: OutputItem) -> Self {
        match value {
            OutputItem::Message { content, .. } => {
                let parts = content
                    .into_iter()
                    .filter_map(|output| match output {
                        OutputItemContent::OutputText { text } => {
                            Some(TextContentPart::OutputText { text })
                        },
                        _ => None,
                    })
                    .collect();
                InputItem::Message {
                    role: Role::Assistant,
                    content: InputMessageContent::ContentParts(parts),
                }
            },
            OutputItem::Reasoning { content, .. } => {
                let reasonings = content
                    .into_iter()
                    .filter_map(|output| match output {
                        OutputItemContent::ReasoningText { text } => {
                            Some(ReasoningText::ReasoningText { text })
                        },
                        _ => None,
                    })
                    .collect();
                InputItem::Reasoning { content: reasonings }
            },
            OutputItem::FunctionCall { call_id, name, arguments } => {
                InputItem::FunctionCall { call_id, name, arguments }
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StreamEventResponse {
    pub id: String,

    pub model: String,

    pub output: Vec<OutputItem>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub created_at: Option<i64>,

    pub status: ResponseStatus,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct StreamEvent {
    #[serde(rename = "type")]
    pub kind: StreamEventKind,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response: Option<StreamEventResponse>,
}

pub type LlmStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, AgentError>> + Send>>;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

impl LlmClient {
    pub fn new(cfg: &Config) -> Result<Self, AgentError> {
        let http = reqwest::Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .build()
            .map_err(|e| AgentError::new(e.to_string()))?;

        Ok(Self { http, endpoint: cfg.endpoint.to_string(), api_key: cfg.api_key.to_string() })
    }

    pub async fn ask_stream(&self, request: Payload) -> Result<LlmStream, AgentError> {
        let response = self
            .http
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .map_err(|e| AgentError::new(e.to_string()))
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(AgentError::new(format!("status: {}. {}", status, body)));
        }

        let events = response
            .bytes_stream() // body -> 字节块流
            .eventsource() // 字节块 -> SSE 事件（增量解析）
            .map(|event| {
                let event = event.map_err(|e| AgentError::new(e.to_string()))?;
                serde_json::from_str::<StreamEvent>(&event.data)
                    .map_err(|e| AgentError::new(e.to_string()))
            });
        Ok(Box::pin(events)) // 装箱 + Pin -> EventStream
    }
}
