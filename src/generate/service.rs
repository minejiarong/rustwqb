use crate::ai::{ChatRequest, LlmError, LlmProvider};
use crate::generate::context::GenerateContextProvider;
use crate::generate::parser::{parse_alpha_exprs, validate_prequeue};
use crate::generate::prompt::PromptBuilder;
use crate::session::WQBSession;
use crate::storage::repository::DataFieldRepository;
use crate::storage::repository::{AlphaDefinition, AlphaRepository, BacktestRepository};
use crate::AppEvent;
use sea_orm::DatabaseConnection;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::sync::mpsc;

#[derive(Clone, Debug)]
pub struct GenerateConfig {
    pub batch_size: usize,
    pub max_insert: usize,
    pub model: String,
    pub interval_sec: u64,
    pub region: Option<String>,
    pub universe: Option<String>,
    pub delay: Option<i32>,
    pub field_sample_size: usize,
    pub auto_backtest: bool,
}

#[derive(Clone, Debug, Default)]
pub struct GenerateResult {
    pub total_lines: usize,
    pub candidates: usize,
    pub accepted: usize,
    pub inserted: usize,
    pub rejected_examples: Vec<String>,
}

pub struct GeneratorService<P: LlmProvider> {
    provider: P,
    db: Arc<DatabaseConnection>,
    session: Arc<WQBSession>,
    evt_tx: mpsc::UnboundedSender<AppEvent>,
    ctx: Arc<dyn GenerateContextProvider>,
}

impl<P: LlmProvider + Clone + Send + Sync + 'static> GeneratorService<P> {
    pub fn new(
        provider: P,
        db: Arc<DatabaseConnection>,
        session: Arc<WQBSession>,
        evt_tx: mpsc::UnboundedSender<AppEvent>,
        ctx: Arc<dyn GenerateContextProvider>,
    ) -> Self {
        Self {
            provider,
            db,
            session,
            evt_tx,
            ctx,
        }
    }

    pub async fn run_loop(&self, cfg: GenerateConfig) {
        loop {
            match self.generate_once(&cfg).await {
                Ok(res) => {
                    let _ = self.evt_tx.send(AppEvent::Log(format!(
                        "生成完成: 候选 {}, 入库 {}, 拒绝 {}",
                        res.candidates,
                        res.inserted,
                        res.rejected_examples.len()
                    )));
                }
                Err(e) => {
                    let _ = self
                        .evt_tx
                        .send(AppEvent::Error(format!("生成出错: {}", e)));
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(cfg.interval_sec)).await;
        }
    }

    pub async fn generate_once(
        &self,
        cfg: &GenerateConfig,
    ) -> Result<GenerateResult, anyhow::Error> {
        let operators = self.ctx.get_operator_catalog().await?;
        let pb = PromptBuilder::new(operators);
        let (non_event_fields, event_fields) = DataFieldRepository::sample_weighted_fields_grouped(
            self.db.as_ref(),
            cfg.region.clone(),
            cfg.universe.clone(),
            cfg.delay,
            cfg.field_sample_size,
        )
        .await?;
        let incompatible_ops_set =
            crate::storage::repository::OperatorCompatRepository::list_incompatible_ops(
                self.db.as_ref(),
            )
            .await
            .unwrap_or_default();
        let mut incompatible_ops: Vec<String> = incompatible_ops_set.into_iter().collect();
        incompatible_ops.sort();
        let prompt = pb.build_with_field_groups(
            cfg.batch_size,
            &non_event_fields,
            &event_fields,
            cfg.region.as_deref(),
            cfg.universe.as_deref(),
            cfg.delay,
            &incompatible_ops,
        );

        let req = ChatRequest {
            model: cfg.model.clone(),
            system: "You generate alpha expressions for WorldQuant BRAIN FASTEXPR. Output only expressions.".to_string(),
            user: prompt,
            temperature: 0.7,
            max_tokens: 2048,
        };

        let resp = match self.provider.chat(req).await {
            Ok(r) => r,
            Err(LlmError::Unauthorized) => {
                let provider =
                    std::env::var("LLM_PROVIDER").unwrap_or_else(|_| "openrouter".to_string());
                let msg = match provider.to_ascii_lowercase().as_str() {
                    "cerebras" => "AI 未授权：请在 .env 设置 CEREBRAS_API_KEY（可选 CEREBRAS_BASE_URL），或切换 LLM_PROVIDER=openrouter 并设置 OPENROUTER_API_KEY",
                    _ => "AI 未授权：请在 .env 设置 OPENROUTER_API_KEY（可选 OPENROUTER_BASE_URL），或切换 LLM_PROVIDER=cerebras 并设置 CEREBRAS_API_KEY",
                };
                return Err(anyhow::anyhow!(msg));
            }
            Err(e) => return Err(anyhow::anyhow!(e.to_string())),
        };
        let parsed = parse_alpha_exprs(&resp.text);
        let candidates_count = parsed.exprs.len();

        let mut seen = HashSet::new();
        let mut accepted = Vec::new();
        for e in &parsed.exprs {
            if accepted.len() >= cfg.max_insert {
                break;
            }
            if seen.insert(e.clone()) {
                accepted.push(e.clone());
            }
        }

        let region = cfg.region.clone().unwrap_or_else(|| "CHN".to_string());
        let universe = cfg
            .universe
            .clone()
            .unwrap_or_else(|| "TOP2000U".to_string());
        let delay = cfg.delay.unwrap_or(1);

        let defs: Vec<AlphaDefinition> = accepted
            .iter()
            .map(|expression| AlphaDefinition {
                expression: expression.clone(),
                region: region.clone(),
                universe: universe.clone(),
                language: "FASTEXPR".to_string(),
                delay,
                decay: 10,
                neutralization: "INDUSTRY".to_string(),
                operator_count: 0,
            })
            .collect();

        let _ = AlphaRepository::insert_batch(self.db.as_ref(), defs).await?;
        if cfg.auto_backtest {
            let mut queued = 0usize;
            for expression in &accepted {
                if let Err(reason) = validate_prequeue(expression) {
                    let msg = match reason.as_str() {
                        "unexpected_right_paren" => "预提交校验失败：存在意外右括号（形如 ...)(...）",
                        "trailing_comma" => "预提交校验失败：存在拖尾逗号（形如 ...,)）",
                        "winsorize_arity" => "预提交校验失败：winsorize 仅接受 1 个输入参数",
                        _ => "预提交校验失败：表达式不符合入队规则",
                    };
                    let _ = self.evt_tx.send(AppEvent::Log(format!("跳过入队：{} => {}", expression, msg)));
                    continue;
                }
                if let Err(crate::storage::repository::data_field_repo::EventOpValidationErr::Incompatible) =
                    DataFieldRepository::validate_event_operator_compatibility(
                        self.db.as_ref(),
                        expression,
                        cfg.region.as_deref(),
                        cfg.universe.as_deref(),
                        cfg.delay,
                    )
                    .await
                {
                    let _ = self.evt_tx.send(AppEvent::Log(format!(
                        "跳过入队：{} => 预提交校验失败：事件字段与不兼容运算符组合",
                        expression
                    )));
                    continue;
                }
                if let Some(_) = BacktestRepository::create_job(
                    self.db.as_ref(),
                    expression.clone(),
                    region.clone(),
                    universe.clone(),
                )
                .await?
                {
                    queued += 1;
                }
            }
            let _ = self
                .evt_tx
                .send(AppEvent::Log(format!("已自动加入回测队列: {}", queued)));
        }
        let inserted = accepted.len();

        Ok(GenerateResult {
            total_lines: parsed.total_lines,
            candidates: candidates_count,
            accepted: accepted.len(),
            inserted,
            rejected_examples: parsed.rejected_examples,
        })
    }
}
