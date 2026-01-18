use crate::storage::repository::CoreMetrics;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum BacktestErrorType {
    Infra,    // 系统/网络/限流/Slot不足（可重试）
    Alpha,    // 表达式错误/因子不存在/逻辑不合法（不可重试）
    Internal, // 本地程序错误/数据库异常（人工介入）
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BacktestError {
    pub error_type: BacktestErrorType,
    pub message: String,
    pub retryable: bool,
}

impl BacktestError {
    pub fn infra(msg: impl Into<String>) -> Self {
        Self {
            error_type: BacktestErrorType::Infra,
            message: msg.into(),
            retryable: true,
        }
    }

    pub fn alpha(msg: impl Into<String>) -> Self {
        Self {
            error_type: BacktestErrorType::Alpha,
            message: msg.into(),
            retryable: false,
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self {
            error_type: BacktestErrorType::Internal,
            message: msg.into(),
            retryable: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct BacktestResult {
    pub alpha_id: Option<String>,
    pub simulation_id: Option<String>,
    pub core_metrics: Option<CoreMetrics>,
    pub metrics_json: Option<Value>,
    pub checks_json: Option<Value>,
}

#[derive(Debug, Clone, Default)]
pub struct BacktestStats {
    pub total: usize,
    pub pending: usize,
    pub running: usize,
    pub completed: usize,
    pub error_retryable: usize,
    pub error_fatal: usize,
    pub error_exceeded: usize, // 新增：超过重试次数的任务
}
