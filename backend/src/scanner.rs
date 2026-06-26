use crate::api::AppState;
use crate::models::{AdResult, PanelLog, ScanResponse, Settings};
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
    let keywords = selected_keywords(&settings, now, force);

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
        let keyword = ad
            .matched_keywords
            .first()
            .cloned()
            .unwrap_or_else(|| "all".to_string());

        if let Some(matched) = white_match {
            let mut log = base_ad_log(
                "warning",
                "Order yuborilmadi: oq ro'yxat",
                format!(
                    "{} oq ro'yxatda bor. Key: {keyword}. Qora ro'yxat mos kelsa ham order yuborilmaydi.",
                    matched.display
                ),
                ad,
                &keyword,
                Some(&matched),
            );
            log.raw_response = Some("SMMMAIN chaqirilmadi, sabab: oq ro'yxat".to_string());
            logs.push(log);
            continue;
        }

        let Some(matched) = black_match else {
            let mut log = base_ad_log(
                "info",
                "Order yuborilmadi: ro'yxatda yo'q",
                format!("Ads topildi, lekin target kanal qora ro'yxatda emas. Key: {keyword}."),
                ad,
                &keyword,
                None,
            );
            log.raw_response =
                Some("SMMMAIN chaqirilmadi, sabab: qora ro'yxatda moslik yo'q".to_string());
            logs.push(log);
            continue;
        };

        let mut log = base_ad_log(
            "info",
            "Qora ro'yxat mos keldi",
            format!(
                "{} qora ro'yxatda topildi. SMMMAIN service {} ga order yuborilmoqda. Quality: {}.",
                matched.display,
                state.smmmain.service_id(),
                settings.order_quantity
            ),
            ad,
            &keyword,
            Some(&matched),
        );
        log.quantity = Some(settings.order_quantity);
        log.service_id = Some(state.smmmain.service_id());

        match state
            .smmmain
            .send_order(&matched.order_link, settings.order_quantity)
            .await
        {
            Ok(outcome) => {
                log.level = "success".to_string();
                log.title = "Order yuborildi".to_string();
                log.message = format!(
                    "{} uchun SMMMAIN order yuborildi. Service: {}, quality: {}.",
                    matched.display,
                    state.smmmain.service_id(),
                    settings.order_quantity
                );
                log.order_id = outcome.order_id;
                log.raw_response = Some(outcome.raw_response);
            }
            Err(err) => {
                log.level = "error".to_string();
                log.title = "Order yuborishda xato".to_string();
                log.message = format!(
                    "{} qora ro'yxatda topildi, lekin SMMMAIN order yuborilmadi: {err}",
                    matched.display
                );
                log.raw_response = Some(err.to_string());
                state.runtime.write().await.last_error = Some(log.message.clone());
            }
        }

        logs.push(log);
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

fn selected_keywords(settings: &Settings, now: DateTime<Utc>, force: bool) -> Vec<String> {
    if settings.keyword_rules.is_empty() {
        return settings
            .keywords
            .iter()
            .map(|keyword| keyword.trim().to_string())
            .filter(|keyword| !keyword.is_empty())
            .collect();
    }

    settings
        .keyword_rules
        .iter()
        .filter(|rule| rule.enabled)
        .filter(|rule| force || rule.next_check_at.map(|next| next <= now).unwrap_or(true))
        .map(|rule| rule.text.trim().to_string())
        .filter(|keyword| !keyword.is_empty())
        .collect()
}
