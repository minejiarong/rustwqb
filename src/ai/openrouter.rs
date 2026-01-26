use crate::ai::build_llm_http_client;
use crate::ai::types::{ChatRequest, ChatResponse, LlmError, LlmProvider};
use async_trait::async_trait;
use reqwest::StatusCode;
use serde_json::Value;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Clone)]
pub struct OpenRouterProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    api_keys: Vec<String>,
    index: Arc<AtomicUsize>,
}

impl OpenRouterProvider {
    pub fn from_env() -> Result<Self, LlmError> {
        let keys_raw = std::env::var("OPENROUTER_API_KEYS").ok();
        let api_keys = keys_raw
            .map(|s| {
                s.split(|c| c == ',' || c == ';' || c == '\n' || c == '\t' || c == ' ')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let api_key = if api_keys.is_empty() {
            std::env::var("OPENROUTER_API_KEY")
                .map_err(|_| LlmError::MissingEnv("OPENROUTER_API_KEY"))?
        } else {
            api_keys[0].clone()
        };
        let base_url = std::env::var("OPENROUTER_BASE_URL")
            .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string());

        Ok(Self {
            client: build_llm_http_client()?,
            api_key,
            base_url,
            api_keys,
            index: Arc::new(AtomicUsize::new(0)),
        })
    }

    pub fn new(api_key: String, _model: String, base_url: String) -> Self {
        let client = build_llm_http_client().unwrap_or_else(|_| reqwest::Client::new());
        Self {
            client,
            api_key,
            base_url,
            api_keys: Vec::new(),
            index: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait]
impl LlmProvider for OpenRouterProvider {
    async fn chat(&self, req: ChatRequest) -> Result<ChatResponse, LlmError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": req.model,
            "temperature": req.temperature,
            "max_tokens": req.max_tokens,
            "messages": [
                {"role": "system", "content": req.system},
                {"role": "user", "content": req.user}
            ]
        });

        let mut resp = None;
        for _ in 0..2 {
            let key = if self.api_keys.is_empty() {
                self.api_key.clone()
            } else {
                let i = self.index.fetch_add(1, Ordering::Relaxed);
                let idx = i % self.api_keys.len();
                self.api_keys[idx].clone()
            };
            match self
                .client
                .post(url.clone())
                .bearer_auth(&key)
                .header("Content-Type", "application/json")
                .json(&body)
                .send()
                .await
            {
                Ok(r) => {
                    resp = Some(r);
                    break;
                }
                Err(e) => {
                    if e.is_timeout() {
                        continue;
                    } else {
                        return Err(LlmError::Http(e.to_string()));
                    }
                }
            }
        }
        let resp = resp.ok_or_else(|| LlmError::Http("timeout".to_string()))?;

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

        // 兼容多种返回结构：message.content（字符串或数组）、content（字符串或数组）、text，以及顶层 output_text
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
        } else if let Some(Value::String(s)) = choice0.get("text") {
            s.clone()
        } else if let Some(Value::String(s)) = v.get("output_text") {
            s.clone()
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
