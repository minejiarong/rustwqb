use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Deserialize, Serialize)]
pub struct SimulationResponse {
    pub id: String,
    pub status: String,
    pub progress: Option<f64>,
    pub alpha: Option<String>,
    pub message: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AlphaDetailResponse {
    pub id: String,
    pub status: String,
    pub settings: Value,
    pub regular: Value,
    pub is: Option<Value>,
    #[serde(rename = "dateCreated")]
    pub date_created: String,
}
