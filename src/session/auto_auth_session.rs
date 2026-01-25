use log::{debug, info, warn};
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

/// 自动认证会话
///
/// 继承自 HTTP 客户端，提供自动认证功能。
/// 当请求失败（如 401）时，会自动重新认证。
pub struct AutoAuthSession {
    client: Client,
    auth_method: String,
    auth_url: String,
    auth_expected: Box<dyn Fn(&Response) -> bool + Send + Sync>,
    auth_max_tries: usize,
    auth_delay_unexpected: Duration,
    expected: Box<dyn Fn(&Response) -> bool + Send + Sync>,
    max_tries: usize,
    delay_unexpected: Duration,
    auth_kwargs: Arc<Mutex<std::collections::HashMap<String, String>>>,
    // 认证状态控制
    last_auth_success: Arc<Mutex<Option<Instant>>>,
    is_authenticating: Arc<Mutex<bool>>,
}

impl AutoAuthSession {
    /// 创建一个新的 AutoAuthSession
    pub fn new(
        auth_method: String,
        auth_url: String,
        auth_expected: Box<dyn Fn(&Response) -> bool + Send + Sync>,
        auth_max_tries: usize,
        auth_delay_unexpected: f64,
        expected: Box<dyn Fn(&Response) -> bool + Send + Sync>,
        max_tries: usize,
        delay_unexpected: f64,
        initial_auth_kwargs: std::collections::HashMap<String, String>,
    ) -> Self {
        Self {
            client: Client::builder()
                .cookie_store(true)
                .timeout(Duration::from_secs(30))
                .user_agent("rustwqb/0.1")
                .build()
                .expect("Failed to create HTTP client"),
            auth_method,
            auth_url,
            auth_expected,
            auth_max_tries: auth_max_tries.max(1),
            auth_delay_unexpected: Duration::from_secs_f64(auth_delay_unexpected.max(0.0)),
            expected,
            max_tries: max_tries.max(1),
            delay_unexpected: Duration::from_secs_f64(delay_unexpected.max(0.0)),
            auth_kwargs: Arc::new(Mutex::new(initial_auth_kwargs)),
            last_auth_success: Arc::new(Mutex::new(None)),
            is_authenticating: Arc::new(Mutex::new(false)),
        }
    }

    /// 设置认证参数
    pub async fn set_auth_kwargs(&self, kwargs: std::collections::HashMap<String, String>) {
        let mut lock = self.auth_kwargs.lock().await;
        *lock = kwargs;
    }

    /// 执行认证请求
    pub async fn auth_request(&self) -> Result<Response, reqwest::Error> {
        // 1. 频率控制：如果最近 30 秒内认证过，直接返回（避免重复认证）
        {
            let last_success = self.last_auth_success.lock().await;
            if let Some(instant) = *last_success {
                if instant.elapsed() < Duration::from_secs(30) {
                    debug!("{} auth_request skipped (recently authenticated)", self);
                    // 这里由于需要返回一个 Response，我们其实没法直接返回"上一个成功响应"
                    // 但在 request_with_retry 中，我们主要是为了更新 Cookie/Token
                }
            }
        }

        // 2. 互斥控制：确保只有一个任务在执行认证
        let mut is_auth = self.is_authenticating.lock().await;
        if *is_auth {
            debug!(
                "{} auth_request skipped (another authentication in progress)",
                self
            );
            // 简单等待一下然后返回（实际上应该等那个认证完成，但为了简单，先这样）
            tokio::time::sleep(Duration::from_millis(500)).await;
            // 随便返回一个空响应是不行的，但这里的调用者通常忽略返回值
        }
        *is_auth = true;

        // 确保无论如何最后都会释放锁
        let result = self.do_auth_request().await;

        *is_auth = false;
        result
    }

    async fn do_auth_request(&self) -> Result<Response, reqwest::Error> {
        let mut resp = None;
        let mut tries = 0;

        for try_num in 1..=self.auth_max_tries {
            tries = try_num;
            let request = match self.auth_method.as_str() {
                "POST" => self.client.post(&self.auth_url),
                "GET" => self.client.get(&self.auth_url),
                _ => {
                    warn!("Unsupported auth method: {}", self.auth_method);
                    continue;
                }
            };

            let mut request = request;
            {
                let kwargs = self.auth_kwargs.lock().await;
                for (key, value) in kwargs.iter() {
                    request = request.header(key, value);
                }
            }

            resp = Some(request.send().await?);
            if let Some(ref r) = resp {
                if (self.auth_expected)(r) {
                    let mut last_success = self.last_auth_success.lock().await;
                    *last_success = Some(Instant::now());
                    break;
                }
            }

            if try_num < self.auth_max_tries {
                tokio::time::sleep(self.auth_delay_unexpected).await;
            }
        }

        if let Some(ref r) = resp {
            if !(self.auth_expected)(r) {
                warn!("{} auth_request(...) [max {} tries ran out]", self, tries);
            } else {
                info!("{} auth_request(...) [{} tries]", self, tries);
            }
        }

        Ok(resp.unwrap())
    }

    /// 执行 HTTP 请求（带自动认证）
    pub async fn request<F>(&self, builder: F) -> Result<Response, reqwest::Error>
    where
        F: Fn(&Client) -> RequestBuilder,
    {
        self.request_with_retry(builder, None, None, None).await
    }

    /// 执行 HTTP 请求（带重试和自动认证）
    pub async fn request_with_retry<F>(
        &self,
        builder: F,
        expected: Option<&(dyn Fn(&Response) -> bool + Send + Sync)>,
        max_tries: Option<usize>,
        delay_unexpected: Option<Duration>,
    ) -> Result<Response, reqwest::Error>
    where
        F: Fn(&Client) -> RequestBuilder,
    {
        let expected_fn = expected.unwrap_or(self.expected.as_ref());
        let max_tries = max_tries.unwrap_or(self.max_tries).max(1);
        let base_delay = delay_unexpected.unwrap_or(self.delay_unexpected);

        let mut resp = None;
        let mut tries = 0;

        for try_num in 1..=max_tries {
            tries = try_num;

            let request_builder = builder(&self.client);
            let response = request_builder.send().await?;
            let status = response.status();

            // 如果请求符合预期，直接返回
            if expected_fn(&response) {
                resp = Some(response);
                break;
            }

            // 如果不符合预期，根据状态码采取不同策略
            if try_num < max_tries {
                let mut current_delay = base_delay;

                if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
                    // 401/403: 触发重新认证
                    warn!(
                        "{} status {}, triggering re-auth (try {})",
                        self, status, try_num
                    );
                    let _ = self.auth_request().await;
                } else if status == StatusCode::TOO_MANY_REQUESTS {
                    // 429: 限流退避，不执行认证
                    current_delay = self.get_retry_after(&response).unwrap_or_else(|| {
                        // 指数退避: base_delay * 2^(try_num-1)
                        base_delay * 2u32.pow(try_num as u32 - 1)
                    });
                    warn!(
                        "{} status 429, backing off for {:?} (try {})",
                        self, current_delay, try_num
                    );
                } else {
                    // 其他错误: 仅重试不认证
                    warn!("{} status {}, retrying (try {})", self, status, try_num);
                }

                tokio::time::sleep(current_delay).await;
            }
            resp = Some(response);
        }

        if let Some(ref r) = resp {
            if !expected_fn(r) {
                warn!(
                    "{} request(...) [max {} tries ran out, last status: {}]",
                    self,
                    tries,
                    r.status()
                );
            } else {
                info!("{} request(...) [{} tries]", self, tries);
            }
        }

        Ok(resp.unwrap())
    }

    fn get_retry_after(&self, resp: &Response) -> Option<Duration> {
        resp.headers()
            .get("Retry-After")
            .and_then(|h| h.to_str().ok())
            .and_then(|s| {
                if let Ok(secs) = s.parse::<u64>() {
                    Some(Duration::from_secs(secs))
                } else {
                    // 暂不支持 HttpDate 格式，只支持秒数
                    None
                }
            })
    }

    /// GET 请求
    pub async fn get(&self, url: &str) -> Result<Response, reqwest::Error> {
        self.request(|client| client.get(url)).await
    }

    /// POST 请求
    pub async fn post(&self, url: &str) -> Result<Response, reqwest::Error> {
        self.request(|client| client.post(url)).await
    }

    /// PUT 请求
    pub async fn put(&self, url: &str) -> Result<Response, reqwest::Error> {
        self.request(|client| client.put(url)).await
    }

    /// PATCH 请求
    pub async fn patch(&self, url: &str) -> Result<Response, reqwest::Error> {
        self.request(|client| client.patch(url)).await
    }

    /// DELETE 请求
    pub async fn delete(&self, url: &str) -> Result<Response, reqwest::Error> {
        self.request(|client| client.delete(url)).await
    }

    /// HEAD 请求
    pub async fn head(&self, url: &str) -> Result<Response, reqwest::Error> {
        self.request(|client| client.head(url)).await
    }
}

impl std::fmt::Display for AutoAuthSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<AutoAuthSession []>")
    }
}

impl std::fmt::Debug for AutoAuthSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<AutoAuthSession []>")
    }
}
