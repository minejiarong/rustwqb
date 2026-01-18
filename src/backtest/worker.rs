use crate::backtest::model::{BacktestError, BacktestResult};
use crate::session::dto::{AlphaDetailResponse, SimulationResponse};
use crate::session::WQBSession;
use crate::storage::repository::CoreMetrics;
use log::info;
use serde_json::Value;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

pub struct BacktestWorker;

impl BacktestWorker {
    /// 执行器：接收表达式，返回结果或分型后的错误
    pub async fn run(
        expression: &str,
        session: Arc<WQBSession>,
        region: &str,
        universe: &str,
    ) -> Result<BacktestResult, BacktestError> {
        // 1. 提交模拟请求
        let sim_data = Self::build_sim_data(expression, region, universe);
        let resp = session
            .post("https://api.worldquantbrain.com/simulations", |b| {
                b.json(&sim_data)
            })
            .await
            .map_err(|e| BacktestError::infra(format!("网络请求失败: {}", e)))?;

        // 处理提交阶段的错误分型
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();

            return match status {
                400 => Err(BacktestError::alpha(format!("表达式不合法: {}", text))),
                401 => Err(BacktestError::infra("认证过期，等待自动重试")),
                429 => Err(BacktestError::infra("触发 WQB 频率限制 (429)")),
                500..=599 => Err(BacktestError::infra(format!("WQB 服务器波动 ({})", status))),
                _ => Err(BacktestError::internal(format!(
                    "未预期的状态码 ({}): {}",
                    status, text
                ))),
            };
        }

        // --- 核心修复：WQB API 201 响应通常不带 Body，ID 在 Location Header 中 ---
        // 尝试从 Location Header 获取 ID
        let location_id = resp
            .headers()
            .get("Location")
            .and_then(|l| l.to_str().ok())
            .and_then(|s| s.split('/').last())
            .map(|s| s.to_string());

        // 尝试读取 Body (兼容性考虑)
        let body_text = resp.text().await.unwrap_or_default();

        let sim_id = if !body_text.trim().is_empty() {
            // 如果 Body 不为空，尝试解析 JSON
            let sim_info: serde_json::Value = serde_json::from_str(&body_text).map_err(|e| {
                BacktestError::internal(format!("JSON 解析失败: {}, 原始报文: {}", e, body_text))
            })?;
            sim_info
                .get("id")
                .and_then(|id| id.as_str())
                .map(|s| s.to_string())
                .or(location_id)
                .ok_or_else(|| BacktestError::internal("API 返回成功但无法获取 Simulation ID"))?
        } else {
            // 如果 Body 为空，直接使用 Location ID
            location_id
                .ok_or_else(|| BacktestError::internal("API 返回空响应且无 Location Header"))?
        };

        info!("▶ 模拟任务已提交: {}", sim_id);

        // 2. 轮询结果 (Polling)
        let mut poll_count = 0;
        let final_alpha_id = loop {
            poll_count += 1;
            let poll_url = format!("https://api.worldquantbrain.com/simulations/{}", sim_id);
            let poll_resp = session
                .get(&poll_url, |r| r)
                .await
                .map_err(|e| BacktestError::infra(format!("轮询网络失败: {}", e)))?;

            // 核心：WQB 在模拟进行中通常返回 200 + Retry-After + body={"progress":...}
            // 完成后一般不再带 Retry-After，并返回完整 simulation 对象（含 status/alpha）
            let has_retry_after = poll_resp.headers().get("Retry-After").is_some();
            let retry_after = poll_resp
                .headers()
                .get("Retry-After")
                .and_then(|h| h.to_str().ok())
                .and_then(|s| s.parse::<u64>().ok())
                .unwrap_or(20);

            let poll_body = poll_resp
                .text()
                .await
                .map_err(|e| BacktestError::internal(format!("读取轮询响应失败: {}", e)))?;

            if poll_body.trim().is_empty() {
                // 极端情况：空 body，按“仍在进行”处理
                sleep(Duration::from_secs(retry_after)).await;
                continue;
            }

            // 先按 Value 解析，兼容 progress-only 的响应
            let poll_val: Value = serde_json::from_str(&poll_body).map_err(|e| {
                BacktestError::internal(format!(
                    "轮询 JSON 解析失败: {}, 原始报文: {}",
                    e, poll_body
                ))
            })?;

            // 如果在进行中（有 Retry-After 或只有 progress），不要按完整 SimulationResponse 强制解析
            if has_retry_after && poll_val.get("status").is_none() {
                if poll_count % 10 == 0 {
                    if let Some(p) = poll_val.get("progress").and_then(|v| v.as_f64()) {
                        info!(
                            "... 任务进度 [{}]: {:.0}% (已轮询 {} 次)",
                            sim_id,
                            p * 100.0,
                            poll_count
                        );
                    } else {
                        info!("... 任务运行中 [{}] (已轮询 {} 次)", sim_id, poll_count);
                    }
                }
                sleep(Duration::from_secs(retry_after)).await;
                continue;
            }

            // 到这里，基本意味着完成/失败（应当包含 status）
            let poll_info: SimulationResponse = serde_json::from_value(poll_val).map_err(|e| {
                BacktestError::internal(format!(
                    "轮询结果结构不匹配: {}, 原始报文: {}",
                    e, poll_body
                ))
            })?;

            match poll_info.status.as_str() {
                "COMPLETE" | "WARNING" => {
                    info!("✓ 模拟完成 [{}]: {}", sim_id, poll_info.status);
                    if let Some(alpha_id) = poll_info.alpha {
                        break alpha_id;
                    } else {
                        return Err(BacktestError::internal("模拟成功但未返回 alpha ID"));
                    }
                }
                "ERROR" | "FAIL" => {
                    let msg = poll_info
                        .message
                        .unwrap_or_else(|| "未知引擎错误".to_string());
                    return Err(BacktestError::alpha(format!("回测失败: {}", msg)));
                }
                "CANCELLED" => {
                    return Err(BacktestError::infra("任务被外部取消"));
                }
                _ => {
                    sleep(Duration::from_secs(retry_after)).await;
                }
            }
        };

        // 3. 抓取 Alpha 详情
        let detail_url = format!("https://api.worldquantbrain.com/alphas/{}", final_alpha_id);
        let detail_resp = session
            .get(&detail_url, |r| r)
            .await
            .map_err(|e| BacktestError::infra(format!("抓取详情失败: {}", e)))?;

        let detail_info: AlphaDetailResponse = detail_resp
            .json()
            .await
            .map_err(|e| BacktestError::internal(format!("详情 JSON 解析失败: {}", e)))?;

        // 4. 解析指标 (核心指标就在 is 对象的顶层，而不是 raw 内部)
        let mut core_metrics = None;
        let mut metrics_json = None;
        let mut checks_json = None;

        if let Some(is_data) = detail_info.is {
            // 完整保存 IS 数据
            metrics_json = Some(serde_json::json!({
                "IS": is_data
            }));

            // 提取核心 IS 指标
            core_metrics = Some(CoreMetrics {
                is_sharpe: is_data.get("sharpe").and_then(|v| v.as_f64()),
                is_fitness: is_data.get("fitness").and_then(|v| v.as_f64()),
                is_turnover: is_data.get("turnover").and_then(|v| v.as_f64()),
                is_returns: is_data.get("returns").and_then(|v| v.as_f64()),
                is_drawdown: is_data.get("drawdown").and_then(|v| v.as_f64()),
                is_pnl: is_data.get("pnl").and_then(|v| v.as_f64()),
            });

            // 提取 checks
            if let Some(checks) = is_data.get("checks") {
                checks_json = Some(checks.clone());
            }
        }

        Ok(BacktestResult {
            alpha_id: Some(final_alpha_id),
            simulation_id: Some(sim_id),
            core_metrics,
            metrics_json,
            checks_json,
        })
    }

    fn build_sim_data(expression: &str, region: &str, universe: &str) -> serde_json::Value {
        serde_json::json!({
            "type": "REGULAR",
            "settings": {
                "instrumentType": "EQUITY",
                "region": region,
                "universe": universe,
                "delay": 1,
                "decay": 10,
                "neutralization": "INDUSTRY",
                "truncation": 0.08,
                "pasteurization": "ON",
                "unitHandling": "VERIFY",
                "nanHandling": "OFF",
                "language": "FASTEXPR",
                "visualization": false
            },
            "regular": expression
        })
    }
}
