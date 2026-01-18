use async_trait::async_trait;

#[derive(Clone, Debug)]
pub struct ChatRequest {
    pub model: String,
    pub system: String,
    pub user: String,
    pub temperature: f32,
    pub max_tokens: u32,
}

#[derive(Clone, Debug)]
pub struct ChatResponse {
    pub text: String,
    pub raw: Option<String>,
}

#[derive(thiserror::Error, Debug)]
pub enum LlmError {
    #[error("missing env {0}")]
    MissingEnv(&'static str),
    #[error("http error: {0}")]
    Http(String),
    #[error("unauthorized")]
    Unauthorized,
    #[error("rate limited")]
    RateLimited,
    #[error("invalid response: {0}")]
    InvalidResponse(String),
}

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LlmError>;
}
