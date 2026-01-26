use crate::ai::cerebras::CerebrasProvider;
use crate::ai::types::{ChatRequest, ChatResponse, LlmError, LlmProvider};
use crate::ai::OpenRouterProvider;
use crate::ai::XirangProvider;
use async_trait::async_trait;

#[derive(Clone)]
pub enum InnerProvider {
    OpenRouter(OpenRouterProvider),
    Cerebras(CerebrasProvider),
    Xirang(XirangProvider),
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
            "xirang" => {
                let p = XirangProvider::from_env()?;
                Ok(Self {
                    inner: InnerProvider::Xirang(p),
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

    pub fn from_env_for_worker(worker_idx: usize) -> Result<Self, LlmError> {
        let which = std::env::var("LLM_PROVIDER")
            .unwrap_or_else(|_| "openrouter".to_string())
            .to_lowercase();
        match which.as_str() {
            "cerebras" => {
                let keys_raw = std::env::var("CEREBRAS_API_KEYS").ok();
                let keys: Vec<String> = keys_raw
                    .map(|s| {
                        s.split(|c| c == ',' || c == ';' || c == '\n' || c == '\t' || c == ' ')
                            .map(|x| x.trim().to_string())
                            .filter(|x| !x.is_empty())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let base_url = std::env::var("CEREBRAS_BASE_URL")
                    .unwrap_or_else(|_| "https://api.cerebras.ai/v1".to_string());
                let key = if keys.is_empty() {
                    std::env::var("CEREBRAS_API_KEY")
                        .map_err(|_| LlmError::MissingEnv("CEREBRAS_API_KEY"))?
                } else {
                    let idx = worker_idx % keys.len();
                    keys[idx].clone()
                };
                let p = CerebrasProvider::new(key, base_url);
                Ok(Self {
                    inner: InnerProvider::Cerebras(p),
                })
            }
            "xirang" => {
                let keys_raw = std::env::var("XIRANG_APP_KEYS").ok();
                let keys: Vec<String> = keys_raw
                    .map(|s| {
                        s.split(|c| c == ',' || c == ';' || c == '\n' || c == '\t' || c == ' ')
                            .map(|x| x.trim().to_string())
                            .filter(|x| !x.is_empty())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let base_url = std::env::var("XIRANG_BASE_URL")
                    .unwrap_or_else(|_| "https://wishub-x6.ctyun.cn/v1".to_string());
                let key = if keys.is_empty() {
                    std::env::var("XIRANG_APP_KEY")
                        .or_else(|_| std::env::var("XIRANG_app_key"))
                        .map_err(|_| LlmError::MissingEnv("XIRANG_APP_KEY"))?
                } else {
                    let idx = worker_idx % keys.len();
                    keys[idx].clone()
                };
                let p = XirangProvider::new(key, base_url);
                Ok(Self {
                    inner: InnerProvider::Xirang(p),
                })
            }
            _ => {
                let keys_raw = std::env::var("OPENROUTER_API_KEYS").ok();
                let keys: Vec<String> = keys_raw
                    .map(|s| {
                        s.split(|c| c == ',' || c == ';' || c == '\n' || c == '\t' || c == ' ')
                            .map(|x| x.trim().to_string())
                            .filter(|x| !x.is_empty())
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let base_url = std::env::var("OPENROUTER_BASE_URL")
                    .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string());
                let key = if keys.is_empty() {
                    std::env::var("OPENROUTER_API_KEY")
                        .map_err(|_| LlmError::MissingEnv("OPENROUTER_API_KEY"))?
                } else {
                    let idx = worker_idx % keys.len();
                    keys[idx].clone()
                };
                let p = OpenRouterProvider::new(key, "unused".to_string(), base_url);
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
            InnerProvider::Xirang(p) => p.chat(req).await,
        }
    }
}
