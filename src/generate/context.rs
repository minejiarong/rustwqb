use crate::session::WQBSession;
use crate::AppEvent;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use tokio::sync::Mutex;
use tokio::time::sleep;

#[derive(Clone, Debug, Default)]
pub struct OperatorCatalog {
    pub by_category: HashMap<String, Vec<OperatorInfo>>,
}

#[derive(Clone, Debug, Default)]
pub struct FieldEntry {
    pub field_id: String,
    pub description: String,
    pub dataset_id: String,
    pub dataset_name: String,
    pub category_id: String,
    pub category_name: String,
    pub subcategory_id: String,
    pub subcategory_name: String,
    pub region: String,
    pub delay: i32,
    pub universe: String,
    pub field_type: String,
}

#[derive(Clone, Debug, Default)]
pub struct FieldCatalog {
    pub entries: Vec<FieldEntry>,
    pub by_category: HashMap<String, Vec<String>>, // category_name -> field_ids
    pub by_dataset: HashMap<String, Vec<String>>,  // dataset_name  -> field_ids
    pub regions: HashSet<String>,
    pub universes: HashSet<String>,
    pub delays: HashSet<i32>,
}

#[derive(Clone, Debug, Default)]
pub struct OperatorInfo {
    pub name: String,
    pub category: String,
    pub op_type: Option<String>,
    pub definition: Option<String>,
    pub description: Option<String>,
    pub scope: Option<Vec<String>>,
    pub documentation: Option<String>,
    pub level: Option<String>,
}

#[async_trait]
pub trait GenerateContextProvider: Send + Sync {
    async fn get_operator_catalog(&self) -> Result<OperatorCatalog>;
    async fn get_field_catalog(
        &self,
        region: &str,
        delay: i32,
        universe: &str,
    ) -> Result<FieldCatalog>;
}

pub struct ApiContextProvider {
    session: Arc<WQBSession>,
    cache: Mutex<Cache>,
    evt_tx: Option<mpsc::UnboundedSender<AppEvent>>,
}

#[derive(Default)]
struct Cache {
    catalog: Option<OperatorCatalog>,
    last_refresh: Option<Instant>,
    fields_cache: HashMap<String, (FieldCatalog, Instant)>,
}

impl ApiContextProvider {
    pub fn new(session: Arc<WQBSession>) -> Self {
        Self {
            session,
            cache: Mutex::new(Cache::default()),
            evt_tx: None,
        }
    }
    pub fn new_with_events(
        session: Arc<WQBSession>,
        evt_tx: mpsc::UnboundedSender<AppEvent>,
    ) -> Self {
        Self {
            session,
            cache: Mutex::new(Cache::default()),
            evt_tx: Some(evt_tx),
        }
    }
}

#[async_trait]
impl GenerateContextProvider for ApiContextProvider {
    async fn get_operator_catalog(&self) -> Result<OperatorCatalog> {
        let mut guard = self.cache.lock().await;
        let ttl = Duration::from_secs(900);
        if let Some(ts) = guard.last_refresh {
            if ts.elapsed() < ttl {
                if let Some(cat) = guard.catalog.clone() {
                    return Ok(cat);
                }
            }
        }

        let resp = self.session.search_operators().await?;
        let body = resp.text().await?;
        let v: Value = serde_json::from_str(&body)?;

        let arr = v
            .get("operators")
            .and_then(|x| x.as_array())
            .or_else(|| v.get("data").and_then(|x| x.as_array()))
            .or_else(|| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut map: HashMap<String, Vec<OperatorInfo>> = HashMap::new();
        for item in arr {
            let cat = item
                .get("category")
                .and_then(|x| x.as_str())
                .unwrap_or("Unknown")
                .to_string();

            let name = match item.get("name").and_then(|x| x.as_str()) {
                Some(s) if !s.trim().is_empty() => s.trim().to_string(),
                _ => continue,
            };

            let op_type = item
                .get("type")
                .and_then(|x| x.as_str())
                .map(|s| s.trim().to_string());
            let definition = item
                .get("definition")
                .and_then(|x| x.as_str())
                .map(|s| s.trim().to_string());
            let description = item
                .get("description")
                .and_then(|x| x.as_str())
                .map(|s| s.trim().to_string());
            let scope = item.get("scope").and_then(|x| x.as_array()).map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.trim().to_string()))
                    .collect::<Vec<String>>()
            });
            let documentation = item
                .get("documentation")
                .and_then(|x| x.as_str())
                .map(|s| s.trim().to_string());
            let level = item
                .get("level")
                .and_then(|x| x.as_str())
                .map(|s| s.trim().to_string());

            map.entry(cat.clone()).or_default().push(OperatorInfo {
                name,
                category: cat,
                op_type,
                definition,
                description,
                scope,
                documentation,
                level,
            });
        }

        let catalog = OperatorCatalog { by_category: map };
        guard.catalog = Some(catalog.clone());
        guard.last_refresh = Some(Instant::now());
        Ok(catalog)
    }

    async fn get_field_catalog(
        &self,
        region: &str,
        delay: i32,
        universe: &str,
    ) -> Result<FieldCatalog> {
        let key = format!("{}:{}:{}", region, delay, universe);
        let mut guard = self.cache.lock().await;
        let ttl = Duration::from_secs(900);
        if let Some((cat, ts)) = guard.fields_cache.get(&key) {
            if ts.elapsed() < ttl {
                if let Some(tx) = &self.evt_tx {
                    let _ = tx.send(AppEvent::Message(format!(
                        "字段缓存命中：{} 个 ({} / {} / {})",
                        cat.entries.len(),
                        region,
                        universe,
                        delay
                    )));
                }
                return Ok(cat.clone());
            }
        }

        let limit = 50usize;
        let mut offset = 0usize;
        let mut entries: Vec<FieldEntry> = Vec::new();
        let mut by_category: HashMap<String, Vec<String>> = HashMap::new();
        let mut by_dataset: HashMap<String, Vec<String>> = HashMap::new();

        if let Some(tx) = &self.evt_tx {
            let _ = tx.send(AppEvent::Message(format!(
                "开始拉取字段 ({} / {} / {}) ...",
                region, universe, delay
            )));
        }
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
                if let Some(tx) = &self.evt_tx {
                    let _ = tx.send(AppEvent::Message(format!(
                        "字段拉取受限 (429)，等待 {}s 后重试 ({} / {} / {})",
                        wait, region, universe, delay
                    )));
                }
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

                let entry = FieldEntry {
                    field_id: field_id.to_string(),
                    description,
                    dataset_id,
                    dataset_name: dataset_name.clone(),
                    category_id,
                    category_name: category_name.clone(),
                    subcategory_id,
                    subcategory_name,
                    region: region.to_string(),
                    delay,
                    universe: universe.to_string(),
                    field_type,
                };
                entries.push(entry);

                if !category_name.is_empty() {
                    by_category
                        .entry(category_name.clone())
                        .or_default()
                        .push(field_id.to_string());
                }
                if !dataset_name.is_empty() {
                    by_dataset
                        .entry(dataset_name.clone())
                        .or_default()
                        .push(field_id.to_string());
                }
            }

            if let Some(tx) = &self.evt_tx {
                let _ = tx.send(AppEvent::Message(format!(
                    "字段拉取进度：已拉取 {} 个，本页 {} ({} / {} / {})",
                    entries.len(),
                    arr_len,
                    region,
                    universe,
                    delay
                )));
            }
            if arr_len < limit {
                break;
            }
            offset += limit;
            if offset >= 30000 {
                break;
            }
            sleep(Duration::from_millis(250)).await; // 轻微节流，避免触发频率限制
        }

        let mut regions = HashSet::new();
        regions.insert(region.to_string());
        let mut universes = HashSet::new();
        universes.insert(universe.to_string());
        let mut delays = HashSet::new();
        delays.insert(delay);

        let catalog = FieldCatalog {
            entries,
            by_category,
            by_dataset,
            regions,
            universes,
            delays,
        };
        guard
            .fields_cache
            .insert(key, (catalog.clone(), Instant::now()));
        Ok(catalog)
    }
}

pub struct EmptyContextProvider;

impl EmptyContextProvider {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl GenerateContextProvider for EmptyContextProvider {
    async fn get_operator_catalog(&self) -> Result<OperatorCatalog> {
        Ok(OperatorCatalog::default())
    }
    async fn get_field_catalog(
        &self,
        _region: &str,
        _delay: i32,
        _universe: &str,
    ) -> Result<FieldCatalog> {
        Ok(FieldCatalog::default())
    }
}
