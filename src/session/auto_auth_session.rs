use log::{info, warn};
use reqwest::{Client, RequestBuilder, Response};
use std::time::Duration;

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
    auth_kwargs: std::collections::HashMap<String, String>,
}

impl AutoAuthSession {
    /// 创建一个新的 AutoAuthSession
    ///
    /// # 参数
    ///
    /// * `auth_method` - 认证请求方法（如 "POST"）
    /// * `auth_url` - 认证 URL
    /// * `auth_expected` - 判断认证是否成功的函数
    /// * `auth_max_tries` - 认证最大重试次数
    /// * `auth_delay_unexpected` - 认证失败时的延迟时间（秒）
    /// * `expected` - 判断普通请求是否成功的函数
    /// * `max_tries` - 普通请求最大重试次数
    /// * `delay_unexpected` - 普通请求失败时的延迟时间（秒）
    pub fn new(
        auth_method: String,
        auth_url: String,
        auth_expected: Box<dyn Fn(&Response) -> bool + Send + Sync>,
        auth_max_tries: usize,
        auth_delay_unexpected: f64,
        expected: Box<dyn Fn(&Response) -> bool + Send + Sync>,
        max_tries: usize,
        delay_unexpected: f64,
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
            auth_kwargs: std::collections::HashMap::new(),
        }
    }

    /// 设置认证参数
    pub fn set_auth_kwargs(&mut self, kwargs: std::collections::HashMap<String, String>) {
        self.auth_kwargs = kwargs;
    }

    /// 执行认证请求
    pub async fn auth_request(&self) -> Result<Response, reqwest::Error> {
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
            for (key, value) in &self.auth_kwargs {
                request = request.header(key, value);
            }

            resp = Some(request.send().await?);
            if let Some(ref r) = resp {
                if (self.auth_expected)(r) {
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
    ///
    /// # 参数
    ///
    /// * `builder` - 一个闭包，接收 Client 并返回 RequestBuilder
    pub async fn request<F>(&self, builder: F) -> Result<Response, reqwest::Error>
    where
        F: Fn(&Client) -> RequestBuilder,
    {
        self.request_with_retry(builder, None, None, None).await
    }

    /// 执行 HTTP 请求（带重试和自动认证）
    ///
    /// # 参数
    ///
    /// * `builder` - 一个闭包，接收 Client 并返回 RequestBuilder
    /// * `expected` - 可选的预期函数，用于判断请求是否成功
    /// * `max_tries` - 可选的最大重试次数
    /// * `delay_unexpected` - 可选的失败延迟时间
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
        let delay_unexpected = delay_unexpected.unwrap_or(self.delay_unexpected);

        let mut resp = None;
        let mut tries = 0;

        for try_num in 1..=max_tries {
            tries = try_num;

            let mut request_builder = builder(&self.client);
            resp = Some(request_builder.send().await?);

            if let Some(ref r) = resp {
                // 如果请求符合预期，直接返回
                if expected_fn(r) {
                    break;
                }

                // 如果请求不符合预期（可能是认证过期），先等待，然后重新认证
                // 这样可以实现"永久会话"的概念，即使过期也会自动重新认证
                if try_num < max_tries {
                    tokio::time::sleep(delay_unexpected).await;
                    // 重新认证（类似 Python 版本的逻辑）
                    let _ = self.auth_request().await;
                }
            } else {
                // 如果没有响应，等待后重试
                if try_num < max_tries {
                    tokio::time::sleep(delay_unexpected).await;
                    let _ = self.auth_request().await;
                }
            }
        }

        if let Some(ref r) = resp {
            if !expected_fn(r) {
                warn!("{} request(...) [max {} tries ran out]", self, tries);
            } else {
                info!("{} request(...) [{} tries]", self, tries);
            }
        }

        Ok(resp.unwrap())
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
