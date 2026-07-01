use crate::models::{
    AdResult, ChannelSegment, KeywordRule, KeywordStat, OrderRecord, PanelLog, PersistedState,
    Settings, TelegramAccount, TelegramSettings,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use std::collections::{HashMap, HashSet};
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

    /// Topilgan reklamalarni saqlaydi. Qaytaradi: (yangi qo'shilganlar,
    /// har bir fingerprint jami necha marta chiqqani).
    pub async fn push_results(
        &self,
        mut incoming: Vec<AdResult>,
    ) -> Result<(Vec<AdResult>, HashMap<String, u64>)> {
        if incoming.is_empty() {
            return Ok((Vec::new(), HashMap::new()));
        }

        let (added_items, counts) = {
            let mut state = self.inner.write().await;
            let mut added_items = Vec::new();
            let mut counts = HashMap::new();

            for item in incoming.drain(..) {
                let count = state
                    .seen_counts
                    .entry(item.fingerprint.clone())
                    .and_modify(|count| *count += 1)
                    .or_insert(1);
                counts.insert(item.fingerprint.clone(), *count);
                if state.seen.insert(item.fingerprint.clone()) {
                    state.results.insert(0, item.clone());
                    added_items.push(item);
                }
            }

            (added_items, counts)
        };

        if !added_items.is_empty() {
            self.save().await?;
        }

        Ok((added_items, counts))
    }

    pub async fn clear_results(&self) -> Result<()> {
        {
            let mut state = self.inner.write().await;
            state.results.clear();
            state.seen.clear();
            state.seen_counts.clear();
            state.ordered.clear();
            state.orders.clear();
        }
        self.save().await
    }

    /// Shu reklamaga (fingerprint) order yuborilganmi.
    pub async fn is_ordered(&self, fingerprint: &str) -> bool {
        self.inner.read().await.ordered.contains(fingerprint)
    }

    /// Reklamani "order yuborildi" deb belgilaydi va diskka saqlaydi.
    pub async fn mark_ordered(&self, fingerprint: &str) -> Result<()> {
        {
            let mut state = self.inner.write().await;
            state.ordered.insert(fingerprint.to_string());
        }
        self.save().await
    }

    /// So'nggi skanда topilgan kanallarni 24 soatlik statistikaga qo'shadi.
    /// Har event: (kalit so'z, kanal username, kanal sarlavhasi).
    pub async fn record_appearances(
        &self,
        events: &[(String, String, Option<String>)],
        now: DateTime<Utc>,
    ) -> Result<()> {
        if events.is_empty() {
            return Ok(());
        }
        let hour = now.timestamp() / 3600;
        let min_hour = hour - 23;
        {
            let mut state = self.inner.write().await;
            for (keyword, channel, title) in events {
                let kw = keyword.trim().to_lowercase();
                let ch = channel.trim().to_lowercase();
                if kw.is_empty() || ch.is_empty() {
                    continue;
                }
                let channels = state.stats.entry(kw).or_default();
                let bucket = channels.entry(ch).or_default();
                if title.is_some() {
                    bucket.title = title.clone();
                }
                *bucket.hourly.entry(hour).or_insert(0) += 1;
                bucket.hourly.retain(|h, _| *h >= min_hour);
            }
            // 24 soatdan eski (bo'sh) kanallar va kalit so'zlarni tozalaymiz.
            for channels in state.stats.values_mut() {
                channels.retain(|_, bucket| {
                    bucket.hourly.retain(|h, _| *h >= min_hour);
                    !bucket.hourly.is_empty()
                });
            }
            state.stats.retain(|_, channels| !channels.is_empty());
        }
        self.save().await
    }

    /// So'nggi 24 soatlik statistikani donut uchun tayyor ko'rinishda qaytaradi.
    /// `whitelist` — hozirgi oq ro'yxat (rang uchun).
    pub async fn stats_24h(&self, whitelist: &[String], now: DateTime<Utc>) -> Vec<KeywordStat> {
        let hour = now.timestamp() / 3600;
        let min_hour = hour - 23;
        let wl: HashSet<String> = whitelist
            .iter()
            .filter_map(|item| crate::telegram::normalize_channel_ref(item))
            .collect();

        let state = self.inner.read().await;
        let mut out: Vec<KeywordStat> = Vec::new();

        for (keyword, channels) in &state.stats {
            let mut segments: Vec<ChannelSegment> = Vec::new();
            let mut total: u64 = 0;

            for (channel, bucket) in channels {
                let count: u64 = bucket
                    .hourly
                    .iter()
                    .filter(|(h, _)| **h >= min_hour)
                    .map(|(_, c)| *c)
                    .sum();
                if count == 0 {
                    continue;
                }
                total += count;
                segments.push(ChannelSegment {
                    channel: channel.clone(),
                    title: bucket.title.clone(),
                    whitelisted: wl.contains(channel),
                    count,
                    percent: 0.0,
                });
            }

            if total == 0 {
                continue;
            }

            let mut whitelist_count: u64 = 0;
            for seg in &mut segments {
                seg.percent = (seg.count as f64) * 100.0 / (total as f64);
                if seg.whitelisted {
                    whitelist_count += seg.count;
                }
            }
            // Katta ulush oldinda tursin.
            segments.sort_by(|a, b| b.count.cmp(&a.count));

            let whitelist_percent = (whitelist_count as f64) * 100.0 / (total as f64);
            out.push(KeywordStat {
                keyword: keyword.clone(),
                total,
                whitelist_percent,
                order_percent: 100.0 - whitelist_percent,
                segments,
            });
        }

        out.sort_by(|a, b| a.keyword.cmp(&b.keyword));
        out
    }

    /// Kalit so'z uchun order slotini atomik band qiladi. Oxirgi orderdan
    /// `min_gap_secs` o'tgan bo'lsa — vaqtni `now` ga o'rnatib `None` qaytaradi
    /// (order yuborishga ruxsat). Aks holda qolgan sekundlarni qaytaradi (rad etildi,
    /// slot band qilinmaydi). Muvaffaqiyat/xato natijadan qat'i nazar shu vaqt hisoblanadi.
    pub async fn reserve_keyword_order(
        &self,
        keyword: &str,
        min_gap_secs: i64,
        now: DateTime<Utc>,
    ) -> Result<Option<i64>> {
        let key = keyword.trim().to_lowercase();
        let remaining = {
            let mut state = self.inner.write().await;
            match state.last_order_at.get(&key) {
                Some(last) => {
                    let elapsed = (now - *last).num_seconds();
                    if elapsed < min_gap_secs {
                        Some((min_gap_secs - elapsed).max(1))
                    } else {
                        state.last_order_at.insert(key, now);
                        None
                    }
                }
                None => {
                    state.last_order_at.insert(key, now);
                    None
                }
            }
        };
        if remaining.is_none() {
            self.save().await?;
        }
        Ok(remaining)
    }

    /// Order yozuvini yangilaydi va diskka saqlaydi (haqiqiy order yuborilganda).
    pub async fn upsert_order_record(&self, record: OrderRecord) -> Result<()> {
        let key = record.link.trim().to_lowercase();
        {
            let mut state = self.inner.write().await;
            state.orders.insert(key, record);
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

    pub async fn accounts(&self) -> Vec<TelegramAccount> {
        self.inner.read().await.accounts.clone()
    }

    pub async fn add_account(&self, account: TelegramAccount) -> Result<()> {
        {
            let mut state = self.inner.write().await;
            state.accounts.push(account);
        }
        self.save().await
    }

    /// Akkauntning Telegram profil ma'lumotlarini yangilaydi (label ham yangilanadi).
    pub async fn update_account_profile(
        &self,
        id: &str,
        me: &crate::telegram::MeInfo,
    ) -> Result<()> {
        let changed = {
            let mut state = self.inner.write().await;
            if let Some(account) = state.accounts.iter_mut().find(|a| a.id == id) {
                account.username = me.username.clone();
                account.first_name = me.first_name.clone();
                account.last_name = me.last_name.clone();
                account.phone = me.phone.clone();
                account.telegram_id = me.telegram_id;
                let full_name = [me.first_name.as_deref(), me.last_name.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                if !full_name.is_empty() {
                    account.label = Some(full_name);
                } else if let Some(u) = &me.username {
                    account.label = Some(format!("@{u}"));
                }
                true
            } else {
                false
            }
        };
        if changed {
            self.save().await?;
        }
        Ok(())
    }

    pub async fn remove_account(&self, id: &str) -> Result<()> {
        {
            let mut state = self.inner.write().await;
            state.accounts.retain(|account| account.id != id);
        }
        self.save().await
    }

    pub async fn set_account_flood(&self, id: &str, until: DateTime<Utc>) -> Result<()> {
        let changed = {
            let mut state = self.inner.write().await;
            if let Some(account) = state.accounts.iter_mut().find(|a| a.id == id) {
                account.flood_until = Some(until);
                true
            } else {
                false
            }
        };
        if changed {
            self.save().await?;
        }
        Ok(())
    }

    /// Akkauntning oxirgi ishlatilgan vaqtini xotirada yangilaydi (diskka yozmaydi).
    pub async fn touch_account_used(&self, id: &str, at: DateTime<Utc>) {
        let mut state = self.inner.write().await;
        if let Some(account) = state.accounts.iter_mut().find(|a| a.id == id) {
            account.last_used_at = Some(at);
        }
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
    let legacy_keywords = normalize_list(std::mem::take(&mut settings.keywords));
    settings.keyword_rules = normalize_keyword_rules(
        settings.keyword_rules,
        &legacy_keywords,
        settings.interval_seconds,
    );
    sync_legacy_keywords(&mut settings);
    settings.channels = normalize_list(settings.channels);
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

fn trim_logs(state: &mut PersistedState) {
    const MAX_LOGS: usize = 1000;
    if state.logs.len() > MAX_LOGS {
        state.logs.truncate(MAX_LOGS);
    }
}
