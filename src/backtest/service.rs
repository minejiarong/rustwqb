use crate::backtest::model::{BacktestError, BacktestResult};
use crate::backtest::worker::BacktestWorker;
use crate::session::WQBSession;
use crate::storage::repository::{AlphaRepository, BacktestRepository};
use crate::AppEvent;
use log::{error, info, warn};
use sea_orm::{DatabaseConnection, EntityTrait};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

pub struct BacktestService {
    db: Arc<DatabaseConnection>,
    session: Arc<WQBSession>,
    evt_tx: mpsc::UnboundedSender<AppEvent>,
    worker_count: usize,
}

impl BacktestService {
    pub fn new(
        db: Arc<DatabaseConnection>,
        session: Arc<WQBSession>,
        evt_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Self {
        Self {
            db,
            session,
            evt_tx,
            worker_count: 10,
        }
    }

    pub async fn add_job(&self, expression: &str) -> Result<Option<i32>, String> {
        BacktestRepository::create_job(
            &self.db,
            expression.to_string(),
            "CHN".to_string(),
            "TOP2000U".to_string(),
        )
        .await
        .map_err(|e| e.to_string())
    }

    /// 启动常驻 workers（并发=worker_count），只要没满就会立刻填上
    pub fn start_workers(&self) {
        for idx in 0..self.worker_count {
            let worker_id = format!("w{}", idx + 1);
            let db = self.db.clone();
            let session = self.session.clone();
            let evt_tx = self.evt_tx.clone();

            tokio::spawn(async move {
                loop {
                    // 1) 原子 claim 下一条可执行任务（QUEUED/RETRY_WAIT 且 next_run_at<=now）
                    let now = chrono::Utc::now().timestamp();
                    let job = match BacktestRepository::claim_next(&db, &worker_id, now).await {
                        Ok(j) => j,
                        Err(e) => {
                            let _ = evt_tx.send(AppEvent::Log(format!("⚠ claim_next 失败: {}", e)));
                            sleep(Duration::from_millis(300)).await;
                            continue;
                        }
                    };

                    let Some(job) = job else {
                        // 没任务就短睡眠，避免空转
                        sleep(Duration::from_millis(300)).await;
                        continue;
                    };

                    let job_id = job.id;
                    let expression = job.expression.clone();
                    let region = job.region.clone();
                    let universe = job.universe.clone();
                    info!(
                        "🚀 [{}] 开始回测任务 [{}]: {} (region: {}, universe: {})",
                        worker_id, job_id, expression, region, universe
                    );

                    // 2) 标记 SUBMITTING
                    let _ = BacktestRepository::mark_status(&db, job_id, "SUBMITTING", None).await;
                    // 同步 Alpha 状态为 SIMULATING（便于 Alpha 列表显示）
                    let _ = AlphaRepository::mark_simulating(&db, &expression, &worker_id).await;

                    // 3) 运行 worker（submit->poll->fetch）
                    let result =
                        BacktestWorker::run(&expression, session.clone(), &region, &universe).await;
                    match result {
                        Ok(res) => {
                            Self::handle_success(&db, job_id, &expression, res, &evt_tx).await;
                        }
                        Err(err) => {
                            Self::handle_error(&db, job_id, err, &evt_tx).await;
                        }
                    }
                }
            });
        }
    }

    /// 处理成功结果：RUNNING -> DONE
    async fn handle_success(
        db: &Arc<DatabaseConnection>,
        job_id: i32,
        expression: &str,
        result: BacktestResult,
        evt_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        info!("✓ 任务执行成功 [{}]: {:?}", job_id, result.alpha_id);

        // 1. 更新回测任务状态 + 结果
        let _ = BacktestRepository::mark_done(
            db,
            job_id,
            result.simulation_id.clone(),
            result.alpha_id.clone(),
            result.metrics_json.clone(),
            result.checks_json.clone(),
        )
        .await;

        // 2. 同步到 Alpha 表 (持久化回测结果)
        // 只有获取到了具体的 alpha_id 且有指标时才同步
        if result.alpha_id.is_some() {
            // 可以在这里进一步提取 worker 返回的更多信息更新到主表
            let _ = AlphaRepository::mark_done(
                db,
                expression,
                result.core_metrics,
                result.metrics_json,
                result.checks_json,
            )
            .await;
        }

        let _ = evt_tx.send(AppEvent::Log(format!("✓ 回测任务完成: {}", expression)));
    }

    /// 处理失败结果：根据错误分型决定流转
    async fn handle_error(
        db: &Arc<DatabaseConnection>,
        job_id: i32,
        err: BacktestError,
        evt_tx: &mpsc::UnboundedSender<AppEvent>,
    ) {
        warn!("✗ 任务执行失败 [{}]: {}", job_id, err.message);

        // 1. 获取当前任务信息以判断重试次数
        let job = match crate::storage::entity::backtest_job::Entity::find_by_id(job_id)
            .one(db.as_ref())
            .await
        {
            Ok(Some(j)) => j,
            _ => {
                error!("找不到任务记录 [{}], 无法处理错误", job_id);
                return;
            }
        };

        // 2. 判断是否可以重试
        let can_retry = err.retryable && job.retry_count < job.max_retries;

        if can_retry {
            // 指数退避（最简：base=5s，cap=600s，带少量 jitter）
            let base = 5u64;
            let cap = 600u64;
            let exp = (1u64 << (job.retry_count as u32).min(10)).saturating_mul(base);
            let mut delay = exp.min(cap);
            // jitter: 0~20%
            delay = delay + (delay / 5) * (rand::random::<u8>() as u64 % 5) / 5;
            let next_run_at = chrono::Utc::now().timestamp() + delay as i64;

            let _ = BacktestRepository::mark_failed_retryable(
                db,
                job_id,
                "RETRYABLE",
                None,
                Some(err.message.clone()),
                next_run_at,
            )
            .await;
            let _ = evt_tx.send(AppEvent::Log(format!(
                "⚠ 任务重试 [{}/{}]: {}",
                job.retry_count + 1,
                job.max_retries,
                job.expression
            )));
        } else {
            let kind = if !err.retryable {
                "PERMANENT"
            } else {
                "RETRY_EXCEEDED"
            };
            let _ = BacktestRepository::mark_failed_permanent(
                db,
                job_id,
                kind,
                None,
                Some(err.message.clone()),
            )
            .await;

            let _ = AlphaRepository::mark_error(db.as_ref(), &job.expression, &err.message).await;
            let _ = evt_tx.send(AppEvent::Log(format!("✗ 回测最终失败: {}", err.message)));
        }
    }

    /// 系统启动时的恢复逻辑：清理中间态
    pub async fn recover(&self) {
        info!("正在执行回测任务恢复程序...");
        match BacktestRepository::reset_stale_jobs(&self.db).await {
            Ok(count) if count > 0 => {
                info!("✓ 成功恢复 {} 个中断的任务", count);
                let _ = self.evt_tx.send(AppEvent::Log(format!(
                    "✓ 系统恢复: {} 个任务重置为等待状态",
                    count
                )));
            }
            Ok(_) => info!("未发现需要恢复的任务"),
            Err(e) => error!("恢复任务时出错: {}", e),
        }

        match AlphaRepository::reset_stale_simulating(&self.db, 600).await {
            Ok(n) if n > 0 => {
                info!("✓ 清理 {} 条过期的 SIMULATING 记录为 PENDING", n);
                let _ = self.evt_tx.send(AppEvent::Log(format!(
                    "✓ Alpha 状态清理: {} 条 SIMULATING 已重置为 PENDING",
                    n
                )));
            }
            Ok(_) => {}
            Err(e) => error!("清理 SIMULATING 状态时出错: {}", e),
        }
    }
}
