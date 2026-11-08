use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt, TryFutureExt};
use serde::{Deserialize, Serialize};
use std::pin::Pin;
use std::time::Duration;

use crate::Config;

#[derive(Serialize, Deserialize, Clone)]
pub struct Payload {
    pub model: String,
    pub instructions: String,
    pub input: Input,
    pub reasoning: Reasoning,
    pub stream: bool,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct Reasoning {
    pub effort: Effort,
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum Input {
    Text(String),
    Items(Vec<InputItem>),
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InputItem {
    Message { role: Role, content: InputMessageContent },
    Reasoning { content: Vec<ReasoningText> },
    FunctionCallOutput { call_id: String, output: String },
    FunctionCall { call_id: String, name: String, arguments: String },
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(untagged)]
pub enum InputMessageContent {
    Text(String),
    ContentParts(Vec<TextContentPart>),
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TextContentPart {
    InputText { text: String },
    OutputText { text: String },
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ReasoningText {
    ReasoningText { text: String },
}

#[derive(Serialize, Deserialize, Clone)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    System,
}

#[derive(Serialize, Deserialize, Clone)]
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

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct LlmError(pub String);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StreamEventKind {
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta,
    #[serde(rename = "response.reasoning_text.delta")]
    ReasoningTextDelta,
    #[serde(rename = "response.output_item.done")]
    OutputItemDone,
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
#[serde(rename_all = "snake_case")]
pub enum OutputItemContentKind {
    OutputText,
    ReasoningText,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OutputItemContent {
    #[serde(rename = "type")]
    pub kind: OutputItemContentKind,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutputItemKind {
    Message,
    Reasoning,
    FunctionCall,
    #[serde(untagged)]
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct OutputItem {
    #[serde(rename = "type")]
    pub kind: OutputItemKind,
    pub id: String,
    content: Vec<OutputItemContent>,
}

impl Into<InputItem> for OutputItem {
    fn into(self) -> InputItem {
        match self.kind {
            OutputItemKind::Message => {
                let parts = self
                    .content
                    .iter()
                    .map(|output| TextContentPart::OutputText { text: output.text.clone() })
                    .collect();
                InputItem::Message {
                    role: Role::Assistant,
                    content: InputMessageContent::ContentParts(parts),
                }
            },
            OutputItemKind::Reasoning => {
                let content = self
                    .content
                    .iter()
                    .map(|c| ReasoningText::ReasoningText { text: c.text.clone() })
                    .collect();
                InputItem::Reasoning { content }
            },
            OutputItemKind::FunctionCall => todo!(),
            OutputItemKind::Other(_) => todo!(),
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

pub type LlmStream = Pin<Box<dyn Stream<Item = Result<StreamEvent, LlmError>> + Send>>;

// pub type LlmStream = Pin<Box<dyn Stream<Item = Result<Value, LlmError>> + Send>>;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(300);

impl LlmClient {
    pub fn new(cfg: &Config) -> Result<Self, LlmError> {
        let http = reqwest::Client::builder()
            .timeout(DEFAULT_TIMEOUT)
            .build()
            .map_err(|e| LlmError(e.to_string()))?;

        Ok(Self { http, endpoint: cfg.endpoint.to_string(), api_key: cfg.api_key.to_string() })
    }

    pub async fn ask_stream(&self, request: Payload) -> Result<LlmStream, LlmError> {
        let response = self
            .http
            .post(&self.endpoint)
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .map_err(|e| LlmError(e.to_string()))
            .await?;

        let status = response.status();
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(LlmError(format!("status: {}. {}", status, body)));
        }

        let events = response
            .bytes_stream() // body -> 字节块流
            .eventsource() // 字节块 -> SSE 事件（增量解析）
            .map(|event| {
                let event = event.map_err(|e| LlmError(e.to_string()))?;
                serde_json::from_str::<StreamEvent>(&event.data)
                    .map_err(|e| LlmError(e.to_string()))
            });
        Ok(Box::pin(events)) // 装箱 + Pin -> EventStream
    }
}
