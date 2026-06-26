use crate::api::AppState;
use crate::models::{
    AdResult, DEFAULT_SMMMAIN_SERVICE_ID, KeywordRule, PanelLog, ScanResponse, Settings,
};
use crate::telegram::normalize_channel_ref;
use anyhow::Result;
use chrono::{DateTime, Duration, Utc};
use tokio::time::{Duration as TokioDuration, sleep};
use tracing::{error, info};

pub async fn scanner_loop(state: AppState) {
    loop {
        let interval = state.store.settings().await.interval_seconds.max(2);
        {
            let mut runtime = state.runtime.write().await;
            runtime.next_run_at = Some(Utc::now() + Duration::seconds(interval as i64));
        }

        sleep(TokioDuration::from_secs(interval)).await;

        let settings = state.store.settings().await;
        if !settings.enabled {
            continue;
        }

        match scan_due(state.clone()).await {
            Ok(result) => info!(
                added = result.added,
                checked_channels = result.checked_channels,
                checked_keywords = result.checked_keywords,
                "telegram ads scan yakunlandi"
            ),
            Err(err) => {
                error!(error = %err, "telegram ads scan xato");
                state.runtime.write().await.last_error = Some(err.to_string());
            }
        }
    }
}

pub async fn scan_once(state: AppState) -> Result<ScanResponse> {
    scan_with_mode(state, true).await
}

async fn scan_due(state: AppState) -> Result<ScanResponse> {
    scan_with_mode(state, false).await
}

async fn scan_with_mode(state: AppState, force: bool) -> Result<ScanResponse> {
    {
        let mut runtime = state.runtime.write().await;
        if runtime.scanning {
            return Ok(ScanResponse {
                added: 0,
                checked_channels: 0,
                checked_keywords: 0,
                message: "Skaner allaqachon ishlayapti".to_string(),
            });
        }
        runtime.scanning = true;
        runtime.last_error = None;
    }

    let result = scan_inner(&state, force).await;

    {
        let mut runtime = state.runtime.write().await;
        runtime.scanning = false;
        runtime.last_run_at = Some(Utc::now());
        if let Err(err) = &result {
            runtime.last_error = Some(err.to_string());
        }
    }

    result
}

async fn scan_inner(state: &AppState, force: bool) -> Result<ScanResponse> {
    let settings = state.store.settings().await;
    let telegram_settings = state.store.telegram_settings().await;

    if settings.channels.is_empty() {
        return Ok(ScanResponse {
            added: 0,
            checked_channels: 0,
            checked_keywords: 0,
            message: "Kanal ro'yxati bo'sh".to_string(),
        });
    }

    let now = Utc::now();
    let active_keywords = active_keyword_count(&settings);
    let order_keys = selected_order_keys(&settings, now, force);
    let keywords = order_keys
        .iter()
        .map(|keyword| keyword.text.clone())
        .collect::<Vec<_>>();

    if keywords.is_empty() {
        let message = if active_keywords == 0 {
            "Keylar ro'yxati bo'sh".to_string()
        } else {
            "Hozircha navbati kelgan key yo'q".to_string()
        };

        return Ok(ScanResponse {
            added: 0,
            checked_channels: 0,
            checked_keywords: 0,
            message,
        });
    }

    let client = state.telegram.ensure_client(&telegram_settings).await?;
    let mut collected = Vec::new();
    let mut scan_logs = vec![PanelLog::new(
        "info",
        "Scan boshlandi",
        format!(
            "{} ta key navbatda: {}. Tekshiriladigan kanallar: {}",
            keywords.len(),
            keywords.join(", "),
            settings.channels.join(", ")
        ),
    )];
    let mut checked = 0usize;

    for channel in &settings.channels {
        checked += 1;
        match state
            .telegram
            .get_sponsored_messages(&client, channel, &keywords)
            .await
        {
            Ok(mut ads) => collected.append(&mut ads),
            Err(err) => {
                let message = format!("{channel}: {err}");
                state.runtime.write().await.last_error = Some(message.clone());
                let mut log = PanelLog::new(
                    "error",
                    "Kanal tekshirishda xato",
                    format!("{channel} kanalidan ads olib bo'lmadi: {err}"),
                );
                log.source_channel = Some(channel.clone());
                scan_logs.push(log);
            }
        }
    }

    let added_items = state.store.push_results(collected).await?;
    let action_logs = process_ad_actions(state, &settings, &added_items).await;
    scan_logs.extend(action_logs);
    state.store.mark_keywords_checked(&keywords, now).await?;

    scan_logs.push(PanelLog::new(
        "success",
        "Scan yakunlandi",
        format!(
            "{} ta key, {checked} ta kanal tekshirildi. {} ta yangi ads topildi.",
            keywords.len(),
            added_items.len()
        ),
    ));
    state.store.push_logs(scan_logs).await?;

    Ok(ScanResponse {
        added: added_items.len(),
        checked_channels: checked,
        checked_keywords: keywords.len(),
        message: format!(
            "{} ta key, {checked} ta kanal tekshirildi, {added} ta yangi natija",
            keywords.len(),
            added = added_items.len()
        ),
    })
}

async fn process_ad_actions(
    state: &AppState,
    settings: &Settings,
    ads: &[AdResult],
) -> Vec<PanelLog> {
    let mut logs = Vec::new();

    for ad in ads {
        let target = ad
            .target_channel
            .clone()
            .or_else(|| normalize_channel_ref(&ad.url));
        let white_match = find_list_match(target.as_deref(), &ad.url, &settings.whitelist_channels);
        let black_match = find_list_match(target.as_deref(), &ad.url, &settings.blacklist_channels);
        let order_keys = matched_order_keys(settings, &ad.matched_keywords);
        let display_keywords = if order_keys.is_empty() {
            "all".to_string()
        } else {
            order_keys
                .iter()
                .map(|key| key.text.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        };

        if let Some(matched) = white_match {
            let mut log = base_ad_log(
                "warning",
                "Order yuborilmadi: oq ro'yxat",
                format!(
                    "{} oq ro'yxatda bor. Key: {display_keywords}. Qora ro'yxat mos kelsa ham order yuborilmaydi.",
                    matched.display
                ),
                ad,
                &display_keywords,
                Some(&matched),
            );
            log.order_link = order_keys.first().map(|key| key.text.clone());
            log.raw_response = Some("SMMMAIN chaqirilmadi, sabab: oq ro'yxat".to_string());
            logs.push(log);
            continue;
        }

        let Some(matched) = black_match else {
            let mut log = base_ad_log(
                "info",
                "Order yuborilmadi: ro'yxatda yo'q",
                format!(
                    "Ads topildi, lekin target kanal qora ro'yxatda emas. Key: {display_keywords}."
                ),
                ad,
                &display_keywords,
                None,
            );
            log.order_link = order_keys.first().map(|key| key.text.clone());
            log.raw_response =
                Some("SMMMAIN chaqirilmadi, sabab: qora ro'yxatda moslik yo'q".to_string());
            logs.push(log);
            continue;
        };

        for order_key in order_keys {
            let mut log = base_ad_log(
                "info",
                "Qora ro'yxat mos keldi",
                format!(
                    "{} qora ro'yxatda topildi. SMMMAIN service {} ga order yuborilmoqda. Link: {}. Quality: {}.",
                    matched.display, order_key.service_id, order_key.text, order_key.quantity
                ),
                ad,
                &order_key.text,
                Some(&matched),
            );
            log.order_link = Some(order_key.text.clone());
            log.quantity = Some(order_key.quantity);
            log.service_id = Some(order_key.service_id);

            match state
                .smmmain
                .send_order(order_key.service_id, &order_key.text, order_key.quantity)
                .await
            {
                Ok(outcome) => {
                    log.level = "success".to_string();
                    log.title = "Order yuborildi".to_string();
                    log.message = format!(
                        "{} uchun SMMMAIN order yuborildi. Link: {}. Service: {}, quality: {}.",
                        matched.display, order_key.text, order_key.service_id, order_key.quantity
                    );
                    log.order_id = outcome.order_id;
                    log.raw_response = Some(outcome.raw_response);
                }
                Err(err) => {
                    log.level = "error".to_string();
                    log.title = "Order yuborishda xato".to_string();
                    log.message = format!(
                        "{} qora ro'yxatda topildi, lekin SMMMAIN order yuborilmadi. Link: {}. Xato: {err}",
                        matched.display, order_key.text
                    );
                    log.raw_response = Some(err.to_string());
                    state.runtime.write().await.last_error = Some(log.message.clone());
                }
            }

            logs.push(log);
        }
    }

    logs
}

fn base_ad_log(
    level: &str,
    title: &str,
    message: String,
    ad: &AdResult,
    keyword: &str,
    matched: Option<&ChannelMatch>,
) -> PanelLog {
    let mut log = PanelLog::new(level, title, message);
    log.keyword = Some(keyword.to_string());
    log.source_channel = Some(format!("@{}", ad.channel));
    log.target_channel = matched.map(|matched| matched.display.clone()).or_else(|| {
        ad.target_channel
            .as_ref()
            .map(|target| display_channel(target))
    });
    log.ad_url = Some(ad.url.clone());
    log.order_link = matched.map(|matched| matched.order_link.clone());
    log
}

#[derive(Clone, Debug)]
struct ChannelMatch {
    display: String,
    order_link: String,
}

#[derive(Clone, Debug)]
struct OrderKey {
    text: String,
    service_id: u64,
    quantity: u64,
}

fn find_list_match(target: Option<&str>, ad_url: &str, list: &[String]) -> Option<ChannelMatch> {
    let mut candidates = Vec::new();

    if let Some(target) = target.and_then(normalize_channel_ref) {
        candidates.push(target);
    }
    if let Some(target) = normalize_channel_ref(ad_url) {
        candidates.push(target);
    }

    for raw in list {
        let Some(normalized) = normalize_channel_ref(raw) else {
            continue;
        };
        if candidates.iter().any(|candidate| candidate == &normalized) {
            return Some(ChannelMatch {
                display: display_channel(&normalized),
                order_link: order_link(raw, &normalized),
            });
        }
    }

    None
}

fn display_channel(normalized: &str) -> String {
    if normalized.starts_with('+') || normalized.starts_with("http") {
        normalized.to_string()
    } else {
        format!("@{normalized}")
    }
}

fn order_link(raw: &str, normalized: &str) -> String {
    let clean = raw.trim();
    if clean.starts_with("http://") || clean.starts_with("https://") {
        clean.to_string()
    } else if clean.starts_with('@') {
        format!("https://t.me/{}", clean.trim_start_matches('@'))
    } else if normalized.starts_with('+') {
        format!("https://t.me/{normalized}")
    } else {
        format!("https://t.me/{normalized}")
    }
}

fn active_keyword_count(settings: &Settings) -> usize {
    if settings.keyword_rules.is_empty() {
        return settings
            .keywords
            .iter()
            .filter(|keyword| !keyword.trim().is_empty())
            .count();
    }

    settings
        .keyword_rules
        .iter()
        .filter(|rule| rule.enabled && !rule.text.trim().is_empty())
        .count()
}

fn selected_order_keys(settings: &Settings, now: DateTime<Utc>, force: bool) -> Vec<OrderKey> {
    if settings.keyword_rules.is_empty() {
        return settings
            .keywords
            .iter()
            .map(|keyword| keyword.trim().to_string())
            .filter(|keyword| !keyword.is_empty())
            .map(|text| OrderKey {
                text,
                service_id: DEFAULT_SMMMAIN_SERVICE_ID,
                quantity: settings.order_quantity,
            })
            .collect();
    }

    settings
        .keyword_rules
        .iter()
        .filter(|rule| rule.enabled)
        .filter(|rule| force || rule.next_check_at.map(|next| next <= now).unwrap_or(true))
        .filter_map(order_key_from_rule)
        .collect()
}

fn matched_order_keys(settings: &Settings, matched_keywords: &[String]) -> Vec<OrderKey> {
    if matched_keywords.is_empty() {
        return Vec::new();
    }

    if settings.keyword_rules.is_empty() {
        return matched_keywords
            .iter()
            .map(|keyword| keyword.trim().to_string())
            .filter(|keyword| !keyword.is_empty())
            .map(|text| OrderKey {
                text,
                service_id: DEFAULT_SMMMAIN_SERVICE_ID,
                quantity: settings.order_quantity,
            })
            .collect();
    }

    matched_keywords
        .iter()
        .filter_map(|keyword| {
            let wanted = keyword.trim();
            settings
                .keyword_rules
                .iter()
                .find(|rule| rule.text.trim().eq_ignore_ascii_case(wanted))
                .and_then(order_key_from_rule)
        })
        .collect()
}

fn order_key_from_rule(rule: &KeywordRule) -> Option<OrderKey> {
    let text = rule.text.trim();
    if text.is_empty() {
        return None;
    }

    Some(OrderKey {
        text: text.to_string(),
        service_id: rule.service_id.max(1),
        quantity: rule.order_quantity.max(1),
    })
}
