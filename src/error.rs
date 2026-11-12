#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct AgentError(pub String);

impl AgentError {
    pub fn new(reason: impl Into<String>) -> AgentError {
        AgentError(reason.into())
    }
}
