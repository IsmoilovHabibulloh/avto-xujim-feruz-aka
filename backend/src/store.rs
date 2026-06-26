use crate::models::{
    AdResult, DEFAULT_SMMMAIN_SERVICE_ID, KeywordRule, PanelLog, PersistedState, Settings,
    TelegramSettings,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use tokio::sync::RwLock;

pub struct Store {
    path: PathBuf,
    inner: RwLock<PersistedState>,
}

impl Store {
    pub async fn load(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.with_context(|| {
                format!("state papkasini yaratib bo'lmadi: {}", parent.display())
            })?;
        }

        let mut state = match tokio::fs::read_to_string(&path).await {
            Ok(raw) => serde_json::from_str(&raw)
                .with_context(|| format!("state JSON buzilgan: {}", path.display()))?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => PersistedState::default(),
            Err(err) => {
                return Err(err).with_context(|| format!("state o'qilmadi: {}", path.display()));
            }
        };
        state.settings = sanitize_settings(state.settings);

        Ok(Self {
            path,
            inner: RwLock::new(state),
        })
    }

    pub async fn snapshot(&self) -> PersistedState {
        self.inner.read().await.clone()
    }

    pub async fn settings(&self) -> Settings {
        self.inner.read().await.settings.clone()
    }

    pub async fn telegram_settings(&self) -> TelegramSettings {
        self.inner.read().await.telegram.clone()
    }

    pub async fn update_settings(&self, settings: Settings) -> Result<Settings> {
        let clean = sanitize_settings(settings);
        {
            let mut state = self.inner.write().await;
            state.settings = clean.clone();
            trim_results(&mut state);
        }
        self.save().await?;
        Ok(clean)
    }

    pub async fn update_telegram(&self, telegram: TelegramSettings) -> Result<TelegramSettings> {
        {
            let mut state = self.inner.write().await;
            state.telegram = telegram.clone();
        }
        self.save().await?;
        Ok(telegram)
    }

    pub async fn push_results(&self, mut incoming: Vec<AdResult>) -> Result<Vec<AdResult>> {
        if incoming.is_empty() {
            return Ok(Vec::new());
        }

        let added_items = {
            let mut state = self.inner.write().await;
            let mut added_items = Vec::new();

            for item in incoming.drain(..) {
                if state.seen.insert(item.fingerprint.clone()) {
                    state.results.insert(0, item.clone());
                    added_items.push(item);
                }
            }

            trim_results(&mut state);
            added_items
        };

        if !added_items.is_empty() {
            self.save().await?;
        }

        Ok(added_items)
    }

    pub async fn clear_results(&self) -> Result<()> {
        {
            let mut state = self.inner.write().await;
            state.results.clear();
            state.seen.clear();
        }
        self.save().await
    }

    pub async fn push_logs(&self, mut logs: Vec<PanelLog>) -> Result<usize> {
        if logs.is_empty() {
            return Ok(0);
        }

        let added = {
            let mut state = self.inner.write().await;
            let added = logs.len();

            while let Some(log) = logs.pop() {
                state.logs.insert(0, log);
            }

            trim_logs(&mut state);
            added
        };

        self.save().await?;
        Ok(added)
    }

    pub async fn clear_logs(&self) -> Result<()> {
        {
            let mut state = self.inner.write().await;
            state.logs.clear();
        }
        self.save().await
    }

    pub async fn mark_keywords_checked(
        &self,
        keywords: &[String],
        checked_at: DateTime<Utc>,
    ) -> Result<()> {
        if keywords.is_empty() {
            return Ok(());
        }

        let wanted = keywords
            .iter()
            .map(|keyword| keyword.trim().to_lowercase())
            .collect::<HashSet<_>>();

        let changed = {
            let mut state = self.inner.write().await;
            let mut changed = false;

            for rule in &mut state.settings.keyword_rules {
                if wanted.contains(&rule.text.trim().to_lowercase()) {
                    rule.last_checked_at = Some(checked_at);
                    rule.next_check_at =
                        Some(checked_at + Duration::seconds(rule.interval_seconds as i64));
                    changed = true;
                }
            }

            changed
        };

        if changed {
            self.save().await?;
        }

        Ok(())
    }

    async fn save(&self) -> Result<()> {
        let state = self.inner.read().await.clone();
        let raw = serde_json::to_vec_pretty(&state)?;
        let tmp = self.path.with_extension("json.tmp");
        tokio::fs::write(&tmp, raw)
            .await
            .with_context(|| format!("state yozilmadi: {}", tmp.display()))?;
        tokio::fs::rename(&tmp, &self.path)
            .await
            .with_context(|| format!("state almashtirilmadi: {}", self.path.display()))?;
        Ok(())
    }
}

fn sanitize_settings(mut settings: Settings) -> Settings {
    settings.interval_seconds = settings.interval_seconds.clamp(2, 3600);
    settings.max_results = settings.max_results.clamp(50, 5000);
    let legacy_keywords = normalize_list(std::mem::take(&mut settings.keywords));
    settings.keyword_rules = normalize_keyword_rules(
        settings.keyword_rules,
        &legacy_keywords,
        settings.interval_seconds,
    );
    sync_legacy_keywords(&mut settings);
    settings.channels = normalize_list(settings.channels);
    settings.blacklist_channels = normalize_list(settings.blacklist_channels);
    settings.whitelist_channels = normalize_list(settings.whitelist_channels);
    settings.order_quantity = settings.order_quantity.clamp(1, 1_000_000);
    settings
}

fn normalize_list(items: Vec<String>) -> Vec<String> {
    let mut out = Vec::new();
    for item in items {
        let cleaned = item.trim().to_string();
        if !cleaned.is_empty() && !out.iter().any(|x| x == &cleaned) {
            out.push(cleaned);
        }
    }
    out
}

fn normalize_keyword_rules(
    rules: Vec<KeywordRule>,
    legacy_keywords: &[String],
    default_interval: u64,
) -> Vec<KeywordRule> {
    let source = if rules.is_empty() {
        legacy_keywords
            .iter()
            .map(|keyword| KeywordRule::new(keyword.clone(), default_interval))
            .collect()
    } else {
        rules
    };

    let mut out = Vec::new();
    let mut seen = HashSet::new();

    for mut rule in source {
        rule.text = rule.text.trim().to_string();
        if rule.text.is_empty() {
            continue;
        }

        let key = rule.text.to_lowercase();
        if !seen.insert(key) {
            continue;
        }

        rule.interval_seconds = rule.interval_seconds.clamp(2, 86_400);
        rule.order_quantity = rule.order_quantity.clamp(1, 1_000_000);
        if rule.service_id == 0 {
            rule.service_id = DEFAULT_SMMMAIN_SERVICE_ID;
        }
        if rule.enabled {
            rule.next_check_at = rule.last_checked_at.map(|last_checked_at| {
                last_checked_at + Duration::seconds(rule.interval_seconds as i64)
            });
        } else {
            rule.next_check_at = None;
        }
        out.push(rule);
    }

    out
}

fn sync_legacy_keywords(settings: &mut Settings) {
    settings.keywords = settings
        .keyword_rules
        .iter()
        .filter(|rule| rule.enabled)
        .map(|rule| rule.text.clone())
        .collect();
}

fn trim_results(state: &mut PersistedState) {
    let max = state.settings.max_results;
    if state.results.len() > max {
        state.results.truncate(max);
    }
}

fn trim_logs(state: &mut PersistedState) {
    const MAX_LOGS: usize = 1000;
    if state.logs.len() > MAX_LOGS {
        state.logs.truncate(MAX_LOGS);
    }
}
