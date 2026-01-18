use crate::ai::cerebras::CerebrasProvider;
use crate::ai::types::{ChatRequest, ChatResponse, LlmError, LlmProvider};
use crate::ai::OpenRouterProvider;
use async_trait::async_trait;

#[derive(Clone)]
pub enum InnerProvider {
    OpenRouter(OpenRouterProvider),
    Cerebras(CerebrasProvider),
}

#[derive(Clone)]
pub struct AnyProvider {
    inner: InnerProvider,
}

impl AnyProvider {
    pub fn from_env() -> Result<Self, LlmError> {
        let which = std::env::var("LLM_PROVIDER")
            .unwrap_or_else(|_| "openrouter".to_string())
            .to_lowercase();
        match which.as_str() {
            "cerebras" => {
                let p = CerebrasProvider::from_env()?;
                Ok(Self {
                    inner: InnerProvider::Cerebras(p),
                })
            }
            _ => {
                let p = OpenRouterProvider::from_env()?;
                Ok(Self {
                    inner: InnerProvider::OpenRouter(p),
                })
            }
        }
    }
}

#[async_trait]
impl LlmProvider for AnyProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LlmError> {
        match &self.inner {
            InnerProvider::OpenRouter(p) => p.chat(req).await,
            InnerProvider::Cerebras(p) => p.chat(req).await,
        }
    }
}
