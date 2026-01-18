use super::auto_auth_session::AutoAuthSession;
use super::urls::*;
use base64::Engine;
use log::info;
use reqwest::{Response, StatusCode};
use std::collections::HashMap;

/// WQB Session - WorldQuant BRAIN 平台的会话
///
/// 继承自 AutoAuthSession，提供 WorldQuant BRAIN 平台的 API 方法。
pub struct WQBSession {
    session: AutoAuthSession,
    email: String,
    password: String,
}

impl WQBSession {
    /// 创建一个新的 WQBSession
    ///
    /// # 参数
    ///
    /// * `email` - 邮箱地址
    /// * `password` - 密码
    pub fn new(email: String, password: String) -> Self {
        let auth_expected = Box::new(|resp: &Response| resp.status() == StatusCode::CREATED);
        let expected = Box::new(|resp: &Response| {
            let status = resp.status();
            status != StatusCode::NO_CONTENT
                && status != StatusCode::UNAUTHORIZED
                && status != StatusCode::TOO_MANY_REQUESTS
        });

        let mut auth_kwargs = HashMap::new();
        auth_kwargs.insert(
            "Authorization".to_string(),
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", email, password))
            ),
        );

        let mut session = AutoAuthSession::new(
            "POST".to_string(),
            URL_AUTHENTICATION.to_string(),
            auth_expected,
            3,
            2.0,
            expected,
            3,
            2.0,
        );
        session.set_auth_kwargs(auth_kwargs);

        Self {
            session,
            email,
            password,
        }
    }

    /// 获取认证信息
    pub fn get_auth(&self) -> (&str, &str) {
        (&self.email, &self.password)
    }

    /// 设置认证信息
    pub fn set_auth(&mut self, email: String, password: String) {
        self.email = email.clone();
        self.password = password.clone();
        let mut auth_kwargs = HashMap::new();
        auth_kwargs.insert(
            "Authorization".to_string(),
            format!(
                "Basic {}",
                base64::engine::general_purpose::STANDARD.encode(format!("{}:{}", email, password))
            ),
        );
        self.session.set_auth_kwargs(auth_kwargs);
    }

    /// 执行认证请求（用于测试连接）
    pub async fn auth_request(&self) -> Result<Response, reqwest::Error> {
        self.session.auth_request().await
    }

    /// 搜索操作符
    pub async fn search_operators(&self) -> Result<Response, reqwest::Error> {
        let url = URL_OPERATORS;
        let resp = self.session.request(|client| client.get(url)).await?;
        info!("{} search_operators(...) [{}]", self, url);
        Ok(resp)
    }

    /// 定位数据集
    pub async fn locate_dataset(&self, dataset_id: &str) -> Result<Response, reqwest::Error> {
        let url = url_datasets_datasetid(dataset_id);
        let resp = self.session.request(|client| client.get(&url)).await?;
        info!("{} locate_dataset(...) [{}]", self, url);
        Ok(resp)
    }

    /// 定位字段
    pub async fn locate_field(&self, field_id: &str) -> Result<Response, reqwest::Error> {
        let url = url_datafields_fieldid(field_id);
        let resp = self.session.request(|client| client.get(&url)).await?;
        info!("{} locate_field(...) [{}]", self, url);
        Ok(resp)
    }

    /// 定位 Alpha
    pub async fn locate_alpha(&self, alpha_id: &str) -> Result<Response, reqwest::Error> {
        let url = url_alphas_alphaid(alpha_id);
        let resp = self.session.request(|client| client.get(&url)).await?;
        info!("{} locate_alpha(...) [{}]", self, url);
        Ok(resp)
    }

    /// 搜索数据集（有限制）
    pub async fn search_datasets_limited(
        &self,
        region: &str,
        delay: i32,
        universe: &str,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Response, reqwest::Error> {
        let limit = limit.unwrap_or(50).min(50).max(1);
        let offset = offset.unwrap_or(0).min(10000 - limit).max(0);

        let params = vec![
            format!("region={}", region),
            format!("delay={}", delay),
            format!("universe={}", universe),
            format!("instrumentType=EQUITY"),
            format!("limit={}", limit),
            format!("offset={}", offset),
        ];

        let url = format!("{}?{}", URL_DATASETS, params.join("&"));
        let resp = self.session.request(|client| client.get(&url)).await?;
        info!("{} search_datasets_limited(...) [{}]", self, url);
        Ok(resp)
    }

    /// 搜索字段（有限制）
    pub async fn search_fields_limited(
        &self,
        region: &str,
        delay: i32,
        universe: &str,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Response, reqwest::Error> {
        let limit = limit.unwrap_or(50).min(50).max(1);
        let offset = offset.unwrap_or(0).min(10000 - limit).max(0);

        let params = vec![
            format!("region={}", region),
            format!("delay={}", delay),
            format!("universe={}", universe),
            format!("instrumentType=EQUITY"),
            format!("limit={}", limit),
            format!("offset={}", offset),
        ];

        let url = format!("{}?{}", URL_DATAFIELDS, params.join("&"));
        let resp = self.session.request(|client| client.get(&url)).await?;
        info!("{} search_fields_limited(...) [{}]", self, url);
        Ok(resp)
    }

    /// 过滤 Alpha（有限制）
    pub async fn filter_alphas_limited(
        &self,
        status: Option<&str>,
        region: Option<&str>,
        delay: Option<i32>,
        universe: Option<&str>,
        limit: Option<usize>,
        offset: Option<usize>,
    ) -> Result<Response, reqwest::Error> {
        let limit = limit.unwrap_or(100).min(100).max(1);
        let offset = offset.unwrap_or(0).min(10000 - limit).max(0);

        let mut params = Vec::new();
        if let Some(s) = status {
            params.push(format!("status={}", s));
        }
        if let Some(r) = region {
            params.push(format!("settings.region={}", r));
        }
        if let Some(d) = delay {
            params.push(format!("settings.delay={}", d));
        }
        if let Some(u) = universe {
            params.push(format!("settings.universe={}", u));
        }
        params.push(format!("limit={}", limit));
        params.push(format!("offset={}", offset));

        let url = format!("{}?{}", URL_USERS_SELF_ALPHAS, params.join("&"));
        let url = url.replace('+', "%2B");
        let resp = self.session.request(|client| client.get(&url)).await?;
        info!("{} filter_alphas_limited(...) [{}]", self, url);
        Ok(resp)
    }

    /// 检查 Alpha 提交状态
    pub async fn check_alpha(&self, alpha_id: &str) -> Result<Response, reqwest::Error> {
        let url = url_alphas_alphaid_check(alpha_id);
        let resp = self.session.get(&url).await?;
        info!("{} check_alpha(...) [{}]", self, url);
        Ok(resp)
    }

    /// 提交 Alpha
    pub async fn submit_alpha(&self, alpha_id: &str) -> Result<Response, reqwest::Error> {
        let url = url_alphas_alphaid_submit(alpha_id);
        let resp = self.session.request(|client| client.post(&url)).await?;
        info!("{} submit_alpha(...) [{}]", self, url);
        Ok(resp)
    }

    /// PATCH 请求（支持传递 JSON 等参数）
    pub async fn patch<F>(&self, url: &str, builder: F) -> Result<Response, reqwest::Error>
    where
        F: Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    {
        self.session
            .request(|client| builder(client.patch(url)))
            .await
    }

    /// POST 请求（支持传递 JSON 等参数）
    pub async fn post<F>(&self, url: &str, builder: F) -> Result<Response, reqwest::Error>
    where
        F: Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    {
        self.session
            .request(|client| builder(client.post(url)))
            .await
    }

    /// GET 请求（支持传递参数等）
    pub async fn get<F>(&self, url: &str, builder: F) -> Result<Response, reqwest::Error>
    where
        F: Fn(reqwest::RequestBuilder) -> reqwest::RequestBuilder,
    {
        self.session
            .request(|client| builder(client.get(url)))
            .await
    }

    /// 列出数据集（无过滤，分页）
    pub async fn list_datasets_basic(
        &self,
        limit: usize,
        offset: usize,
    ) -> Result<Response, reqwest::Error> {
        let limit = limit.min(50).max(1);
        let url = format!("{}?limit={}&offset={}", URL_DATASETS, limit, offset);
        let resp = self.session.request(|client| client.get(&url)).await?;
        info!("{} list_datasets_basic(...) [{}]", self, url);
        Ok(resp)
    }
}

impl std::fmt::Display for WQBSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<WQBSession [{}]>", self.email)
    }
}

impl std::fmt::Debug for WQBSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "<WQBSession [{}]>", self.email)
    }
}
