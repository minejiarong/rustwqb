use crate::ai::build_llm_http_client;
use crate::ai::types::{ChatRequest, ChatResponse, LlmError, LlmProvider};
use async_trait::async_trait;
use reqwest::StatusCode;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct XirangProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    api_keys: Vec<String>,
    index: Arc<AtomicUsize>,
}

impl XirangProvider {
    pub fn from_env() -> Result<Self, LlmError> {
        let keys_raw = std::env::var("XIRANG_APP_KEYS").ok();
        let api_keys = keys_raw
            .map(|s| {
                s.split(|c| c == ',' || c == ';' || c == '\n' || c == '\t' || c == ' ')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let api_key = if api_keys.is_empty() {
            std::env::var("XIRANG_APP_KEY")
                .or_else(|_| std::env::var("XIRANG_app_key"))
                .map_err(|_| LlmError::MissingEnv("XIRANG_APP_KEY"))?
        } else {
            api_keys[0].clone()
        };
        let base_url = std::env::var("XIRANG_BASE_URL")
            .unwrap_or_else(|_| "https://wishub-x6.ctyun.cn/v1".to_string());
        Ok(Self {
            client: build_llm_http_client()?,
            api_key,
            base_url,
            api_keys,
            index: Arc::new(AtomicUsize::new(0)),
        })
    }
}

#[async_trait]
impl LlmProvider for XirangProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LlmError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": req.model,
            "temperature": req.temperature,
            "max_tokens": req.max_tokens,
            "messages": [
                {"role": "system", "content": req.system},
                {"role": "user", "content": req.user}
            ],
            "stream": false
        });

        let key = if self.api_keys.is_empty() {
            self.api_key.clone()
        } else {
            let i = self.index.fetch_add(1, Ordering::Relaxed);
            let idx = i % self.api_keys.len();
            self.api_keys[idx].clone()
        };

        let resp = self
            .client
            .post(url)
            .bearer_auth(&key)
            .header("Content-Type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?;

        match resp.status() {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => return Err(LlmError::Unauthorized),
            StatusCode::TOO_MANY_REQUESTS => return Err(LlmError::RateLimited),
            _ => {}
        }

        let status = resp.status();
        let raw = resp
            .text()
            .await
            .map_err(|e| LlmError::Http(e.to_string()))?;
        if !status.is_success() {
            return Err(LlmError::Http(format!("{} {}", status.as_u16(), raw)));
        }

        let v: Value = serde_json::from_str(&raw)
            .map_err(|e| LlmError::InvalidResponse(format!("json parse failed: {e}, raw={raw}")))?;
        let choice0 = v
            .get("choices")
            .and_then(|c| c.get(0))
            .ok_or_else(|| LlmError::InvalidResponse(format!("missing choices[0], raw={raw}")))?;
        let content = choice0
            .get("message")
            .and_then(|m| m.get("content"))
            .or_else(|| choice0.get("content"));
        let text = if let Some(content) = content {
            match content {
                Value::String(s) => s.clone(),
                Value::Array(arr) => {
                    let mut parts = Vec::new();
                    for it in arr {
                        if let Some(t) = it.get("text").and_then(|x| x.as_str()) {
                            parts.push(t.to_string());
                        } else if let Some(t) = it.as_str() {
                            parts.push(t.to_string());
                        }
                    }
                    parts.join("\n")
                }
                _ => {
                    return Err(LlmError::InvalidResponse(format!(
                        "unexpected content type, raw={raw}"
                    )))
                }
            }
        } else {
            return Err(LlmError::InvalidResponse(format!(
                "missing content/text in choices[0], raw={raw}"
            )));
        };

        Ok(ChatResponse {
            text,
            raw: Some(raw),
        })
    }
}
