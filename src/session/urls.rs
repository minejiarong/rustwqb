/// WorldQuant BRAIN API 基础 URL
pub const WQB_API_URL: &str = "https://api.worldquantbrain.com";

/// Alpha 相关 URL
pub const URL_ALPHAS: &str = "https://api.worldquantbrain.com/alphas";
pub fn url_alphas_alphaid(alpha_id: &str) -> String {
    format!("{}/{}", URL_ALPHAS, alpha_id)
}
pub fn url_alphas_alphaid_check(alpha_id: &str) -> String {
    format!("{}/{}/check", URL_ALPHAS, alpha_id)
}
pub fn url_alphas_alphaid_submit(alpha_id: &str) -> String {
    format!(
        "https://api.worldquantbrain.com:443/alphas/{}/submit",
        alpha_id
    )
}

/// 认证相关 URL
pub const URL_AUTHENTICATION: &str = "https://api.worldquantbrain.com/authentication";

/// 数据集相关 URL
pub const URL_DATASETS: &str = "https://api.worldquantbrain.com/data-sets";
pub fn url_datasets_datasetid(dataset_id: &str) -> String {
    format!("{}/{}", URL_DATASETS, dataset_id)
}

/// 数据字段相关 URL
pub const URL_DATAFIELDS: &str = "https://api.worldquantbrain.com/data-fields";
pub fn url_datafields_fieldid(field_id: &str) -> String {
    format!("{}/{}", URL_DATAFIELDS, field_id)
}

/// 操作符相关 URL
pub const URL_OPERATORS: &str = "https://api.worldquantbrain.com/operators";

/// 模拟相关 URL
pub const URL_SIMULATIONS: &str = "https://api.worldquantbrain.com/simulations";

/// 用户相关 URL
pub const URL_USERS_SELF: &str = "https://api.worldquantbrain.com/users/self";
pub const URL_USERS_SELF_ALPHAS: &str = "https://api.worldquantbrain.com/users/self/alphas";
