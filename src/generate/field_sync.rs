use crate::generate::context::{
    ApiContextProvider, FieldCatalog, FieldEntry, GenerateContextProvider,
};
use crate::session::WQBSession;
use crate::storage::repository::DataFieldRepository;
use crate::AppEvent;
use anyhow::Result;
use log::{info, warn};
use sea_orm::DatabaseConnection;
use serde_json::Value;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

pub struct FieldSyncService {
    session: Arc<WQBSession>,
    db: Arc<DatabaseConnection>,
    evt_tx: mpsc::UnboundedSender<AppEvent>,
    running: AtomicBool,
}

impl FieldSyncService {
    pub fn new(
        session: Arc<WQBSession>,
        db: Arc<DatabaseConnection>,
        evt_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Self {
        Self {
            session,
            db,
            evt_tx,
            running: AtomicBool::new(false),
        }
    }

    pub async fn discover_regions_universes(&self) -> Result<(BTreeSet<String>, BTreeSet<String>)> {
        let mut regions = BTreeSet::new();
        let mut universes = BTreeSet::new();
        let mut offset = 0usize;
        let limit = 50usize;
        let mut retry = 0u32;
        let max_retry = 5u32;

        let _ = self.evt_tx.send(AppEvent::Message(
            "开始发现可用 Region/Universe...".to_string(),
        ));
        loop {
            let resp = self.session.list_datasets_basic(limit, offset).await?;
            let status = resp.status();
            if status.as_u16() == 429 {
                let wait = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(3);
                retry += 1;
                let _ = self.evt_tx.send(AppEvent::Message(format!(
                    "发现阶段受到频率限制 (429)，等待 {}s 后重试，第 {}/{} 次",
                    wait, retry, max_retry
                )));
                if retry > max_retry {
                    let _ = self.evt_tx.send(AppEvent::Error(
                        "发现阶段重试次数过多，停止发现".to_string(),
                    ));
                    break;
                }
                sleep(Duration::from_secs(wait)).await;
                continue;
            } else {
                retry = 0;
            }
            let body = resp.text().await?;
            let v: Value = serde_json::from_str(&body)?;
            let arr = v
                .get("data")
                .and_then(|x| x.as_array())
                .or_else(|| v.get("results").and_then(|x| x.as_array()))
                .or_else(|| v.as_array())
                .cloned()
                .unwrap_or_default();
            if arr.is_empty() {
                break;
            }
            let arr_len = arr.len();
            info!(
                "字段同步: 扫描数据集 offset={} 批量条数={}",
                offset, arr_len
            );
            for item in arr {
                if let Some(r) = item.get("region").and_then(|x| x.as_str()) {
                    if !r.is_empty() {
                        regions.insert(r.to_string());
                    }
                }
                if let Some(u) = item.get("universe").and_then(|x| x.as_str()) {
                    if !u.is_empty() {
                        universes.insert(u.to_string());
                    }
                }
                if let Some(settings) = item.get("settings") {
                    if let Some(r) = settings.get("region").and_then(|x| x.as_str()) {
                        if !r.is_empty() {
                            regions.insert(r.to_string());
                        }
                    }
                    if let Some(u) = settings.get("universe").and_then(|x| x.as_str()) {
                        if !u.is_empty() {
                            universes.insert(u.to_string());
                        }
                    }
                }
            }
            if arr_len < limit {
                break;
            }
            offset += limit;
            if offset >= 10000 {
                break;
            }
            sleep(Duration::from_millis(250)).await; // 轻微节流，避免触发频率限制
        }

        let _ = self.evt_tx.send(AppEvent::Message(format!(
            "发现完成：regions={}，universes={}",
            regions.len(),
            universes.len()
        )));
        Ok((regions, universes))
    }

    pub async fn sync_combo(
        &self,
        region: &str,
        delay: i32,
        universe: &str,
    ) -> Result<(usize, usize)> {
        let _ = self.evt_tx.send(AppEvent::Message(format!(
            "同步组合：region={} universe={} delay={}",
            region, universe, delay
        )));
        let mut offset = 0usize;
        let limit = 50usize;
        let mut total_inserted = 0usize;
        let mut total_updated = 0usize;
        loop {
            let resp = self
                .session
                .search_fields_limited(region, delay, universe, Some(limit), Some(offset))
                .await?;
            let status = resp.status();
            if status.as_u16() == 429 {
                let wait = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok())
                    .unwrap_or(3);
                let _ = self.evt_tx.send(AppEvent::Message(format!(
                    "字段拉取受限 (429)，等待 {}s 后重试 ({} / {} / {})",
                    wait, region, universe, delay
                )));
                sleep(Duration::from_secs(wait)).await;
                continue;
            }
            let body = resp.text().await?;
            let v: Value = serde_json::from_str(&body)?;
            let arr = v
                .get("fields")
                .and_then(|x| x.as_array())
                .or_else(|| v.get("data").and_then(|x| x.as_array()))
                .or_else(|| v.get("results").and_then(|x| x.as_array()))
                .or_else(|| v.as_array())
                .cloned()
                .unwrap_or_default();
            if arr.is_empty() {
                break;
            }
            let arr_len = arr.len();
            let mut entries: Vec<FieldEntry> = Vec::with_capacity(arr_len);
            for item in arr {
                let field_id = item
                    .get("id")
                    .and_then(|x| x.as_str())
                    .or_else(|| item.get("fieldId").and_then(|x| x.as_str()))
                    .unwrap_or("");
                if field_id.is_empty() {
                    continue;
                }
                let description = item
                    .get("description")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                let (dataset_id, dataset_name) = match item.get("dataset") {
                    Some(d) => (
                        d.get("id")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        d.get("name")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                    ),
                    None => (
                        item.get("datasetId")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        item.get("datasetName")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                    ),
                };
                let (category_id, category_name) = match item.get("category") {
                    Some(c) => (
                        c.get("id")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        c.get("name")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                    ),
                    None => (
                        item.get("categoryId")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        item.get("categoryName")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                    ),
                };
                let (subcategory_id, subcategory_name) = match item.get("subcategory") {
                    Some(c) => (
                        c.get("id")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        c.get("name")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                    ),
                    None => (
                        item.get("subcategoryId")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                        item.get("subcategoryName")
                            .and_then(|x| x.as_str())
                            .unwrap_or("")
                            .to_string(),
                    ),
                };
                let field_type = item
                    .get("type")
                    .and_then(|x| x.as_str())
                    .unwrap_or("")
                    .to_string();
                entries.push(FieldEntry {
                    field_id: field_id.to_string(),
                    description,
                    dataset_id,
                    dataset_name,
                    category_id,
                    category_name,
                    subcategory_id,
                    subcategory_name,
                    region: region.to_string(),
                    delay,
                    universe: universe.to_string(),
                    field_type,
                });
            }
            let (inserted, updated) =
                DataFieldRepository::upsert_batch(self.db.as_ref(), entries.clone()).await?;
            let _ = DataFieldRepository::upsert_scopes(self.db.as_ref(), &entries).await;
            total_inserted += inserted;
            total_updated += updated;
            let _ = self.evt_tx.send(AppEvent::Message(format!(
                "同步分页：本页 {}，插入 {}，更新 {} ({} / {} / {})",
                arr_len, inserted, updated, region, universe, delay
            )));
            if let Ok(rows) =
                DataFieldRepository::stats_by_region_universe_delay(self.db.as_ref()).await
            {
                let _ = self.evt_tx.send(AppEvent::FieldStatsRows(rows));
            }
            if arr_len < limit {
                break;
            }
            offset += limit;
            if offset >= 30000 {
                break;
            }
            sleep(Duration::from_millis(250)).await;
        }
        let _ = self.evt_tx.send(AppEvent::Message(format!(
            "同步完成：累计 插入 {}，更新 {} ({} / {} / {})",
            total_inserted, total_updated, region, universe, delay
        )));
        Ok((total_inserted, total_updated))
    }

    pub async fn sync_all_discovered(&self, delays: &[i32]) -> Result<()> {
        if self.running.swap(true, Ordering::SeqCst) {
            let _ = self.evt_tx.send(AppEvent::Message(
                "已有字段同步任务进行中，忽略本次请求".to_string(),
            ));
            return Ok(());
        }
        let (regions, universes) = self.discover_regions_universes().await?;
        let mut _inserted_total = 0usize;
        let mut _updated_total = 0usize;

        let total = regions.len() * universes.len() * delays.len();
        let _ = self.evt_tx.send(AppEvent::Message(format!(
            "开始字段同步，总组合数：{} (regions={} universes={} delays={})",
            total,
            regions.len(),
            universes.len(),
            delays.len()
        )));
        let mut done = 0usize;
        for r in regions.iter() {
            for u in universes.iter() {
                for &d in delays.iter() {
                    if let Ok((ins, upd)) = self.sync_combo(r, d, u).await {
                        _inserted_total += ins;
                        _updated_total += upd;
                    }
                    done += 1;
                    let pct = (done as f64 / total.max(1) as f64) * 100.0;
                    let _ = self.evt_tx.send(AppEvent::Message(format!(
                        "进度：{}/{} ({:.1}%)，累计 插入 {}，更新 {}",
                        done, total, pct, _inserted_total, _updated_total
                    )));
                }
            }
        }

        let _ = self.evt_tx.send(AppEvent::Message(format!(
            "字段同步完成：插入 {}，更新 {}，组合数 {}",
            _inserted_total, _updated_total, total
        )));
        self.running.store(false, Ordering::SeqCst);
        Ok(())
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }
}
