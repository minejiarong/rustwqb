pub mod cerebras;
pub mod openrouter;
pub mod types;
pub mod unified;

pub use cerebras::CerebrasProvider;
pub use openrouter::OpenRouterProvider;
pub use types::{ChatRequest, ChatResponse, LlmError, LlmProvider};
pub use unified::AnyProvider;

pub(crate) fn build_llm_http_client() -> Result<reqwest::Client, LlmError> {
    let mut builder = reqwest::Client::builder();

    if let Ok(raw) = std::env::var("LLM_PROXY") {
        let t = raw.trim();
        if !t.is_empty() {
            let url = if t.contains("://") {
                t.to_string()
            } else {
                format!("socks5h://{}", t)
            };
            let proxy = reqwest::Proxy::all(&url).map_err(|e| LlmError::Http(e.to_string()))?;
            builder = builder.proxy(proxy);
        }
    }

    builder.build().map_err(|e| LlmError::Http(e.to_string()))
}
