use crate::api::AppState;
use crate::models::{
    AdResult, DEFAULT_SMMMAIN_SERVICE_ID, KeywordRule, MIN_ORDER_GAP_SECS, OrderRecord, PanelLog,
    ScanResponse, Settings,
};
use crate::telegram::normalize_channel_ref;
use anyhow::{Result, anyhow};
use chrono::{DateTime, Duration, Utc};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::Ordering;
use tokio::time::{Duration as TokioDuration, sleep};
use tracing::{error, info};

/// anyhow xato zanjiridan FLOOD_WAIT sekundlarini ajratib oladi (bo'lsa).
fn flood_wait_secs(err: &anyhow::Error) -> Option<i64> {
    let text = format!("{err:?}");
    let pos = text.find("FLOOD_WAIT")?;
    let digits: String = text[pos..]
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    Some(digits.parse::<i64>().unwrap_or(60).max(1))
}

pub async fn scanner_loop(state: AppState) {
    loop {
        let interval = state.store.settings().await.interval_seconds.max(2);
        {
            let mut runtime = state.runtime.write().await;
            runtime.next_run_at = Some(Utc::now() + Duration::seconds(interval as i64));
        }

        sleep(TokioDuration::from_secs(interval)).await;

        // Global yoqish/o'chirish yo'q — har bir key o'z switch'i va jadvali
        // bo'yicha ishlaydi (scan_due faqat navbati kelgan yoqiq keylarni oladi).
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

/// Qo'lda scan: `only_keyword` berilsa faqat o'sha key tekshiriladi
/// (yoqilgan-o'chirilganидан va jadvalidan qat'i nazar).
pub async fn scan_once(state: AppState, only_keyword: Option<String>) -> Result<ScanResponse> {
    scan_with_mode(state, true, only_keyword).await
}

async fn scan_due(state: AppState) -> Result<ScanResponse> {
    scan_with_mode(state, false, None).await
}

async fn scan_with_mode(
    state: AppState,
    force: bool,
    only_keyword: Option<String>,
) -> Result<ScanResponse> {
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

    // scan_inner'ni alohida taskда ishga tushiramiz: agar u panik bersa ham
    // `scanning` bayrog'i quyida albatta tiklanadi. Aks holda bitta panik skanerni
    // abadiy "ishlayapti" holatida qoldirib, undan keyingi barcha skanlarni bloklardi.
    let task_state = state.clone();
    let join =
        tokio::spawn(async move { scan_inner(&task_state, force, only_keyword).await }).await;

    {
        let mut runtime = state.runtime.write().await;
        runtime.scanning = false;
        runtime.last_run_at = Some(Utc::now());
        match &join {
            Ok(Ok(_)) => {}
            Ok(Err(err)) => runtime.last_error = Some(err.to_string()),
            Err(join_err) => runtime.last_error = Some(format!("Skaner ichki xatosi: {join_err}")),
        }
    }

    match join {
        Ok(inner) => inner,
        Err(join_err) => Err(anyhow!("Skaner ichki xatosi: {join_err}")),
    }
}

async fn scan_inner(
    state: &AppState,
    force: bool,
    only_keyword: Option<String>,
) -> Result<ScanResponse> {
    let settings = state.store.settings().await;
    let telegram_settings = state.store.telegram_settings().await;

    let now = Utc::now();
    let active_keywords = active_keyword_count(&settings);
    let order_keys = selected_order_keys(&settings, now, force, only_keyword.as_deref());
    let keywords = order_keys
        .iter()
        .map(|keyword| keyword.text.clone())
        .collect::<Vec<_>>();

    if keywords.is_empty() {
        let message = if let Some(only) = &only_keyword {
            format!("\"{only}\" nomli key topilmadi")
        } else if active_keywords == 0 {
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

    let api_id = telegram_settings
        .api_id
        .ok_or_else(|| anyhow!("Telegram API ID kiritilmagan"))?;
    let accounts = state.store.accounts().await;
    if accounts.is_empty() {
        return Ok(ScanResponse {
            added: 0,
            checked_channels: 0,
            checked_keywords: 0,
            message: "Userbot akkaunt yo'q. QR orqali akkaunt qo'shing".to_string(),
        });
    }

    let mut collected = Vec::new();
    // Har bir key qaysi akkaunt tomonidan qidirilganini eslab qolamiz (log uchun).
    let mut query_accounts: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut start_log = PanelLog::new(
        "info",
        "Scan boshlandi",
        format!(
            "{} ta key bo'yicha global qidiruv boshlandi.",
            keywords.len()
        ),
    );
    start_log.keyword = Some(keywords.join(", "));
    start_log.source_channel = Some("Global qidiruv".to_string());
    start_log.raw_response = Some(format!(
        "{} ta akkaunt navbatda (round-robin)",
        accounts.len()
    ));
    let mut scan_logs = vec![start_log];

    for query in &keywords {
        // Har key uchun navbatdagi (round-robin) sog'lom akkauntni tanlaymiz.
        // FLOOD_WAIT bo'lsa, o'sha akkaunt "dam oladi" va keyingisiga o'tamiz.
        let mut attempts = 0usize;
        loop {
            if attempts >= accounts.len() {
                let mut log = PanelLog::new(
                    "warning",
                    "Akkaunt topilmadi",
                    format!("'{query}' uchun bo'sh (limitsiz) akkaunt yo'q, o'tkazib yuborildi."),
                );
                log.keyword = Some(query.clone());
                log.source_channel = Some("Global qidiruv".to_string());
                log.raw_response = Some("Barcha akkauntlar limitda yoki ulanmagan".to_string());
                scan_logs.push(log);
                break;
            }
            attempts += 1;

            let idx = state.rr.fetch_add(1, Ordering::Relaxed) % accounts.len();
            let account = &accounts[idx];

            if let Some(until) = account.flood_until {
                if until > now {
                    continue; // bu akkaunt hali dam olyapti
                }
            }

            let client = match state
                .telegram
                .ensure_account_client(&account.id, api_id)
                .await
            {
                Ok(client) => client,
                Err(err) => {
                    let label = account.label.clone().unwrap_or_else(|| account.id.clone());
                    let mut log = PanelLog::new(
                        "warning",
                        "Akkaunt ulanmadi",
                        format!("{label} akkaunt ulanmadi, keyingisiga o'tildi."),
                    );
                    log.keyword = Some(query.clone());
                    log.source_channel = Some(label);
                    log.raw_response = Some(err.to_string());
                    scan_logs.push(log);
                    continue;
                }
            };

            match state.telegram.get_sponsored_peers(&client, query).await {
                Ok(mut ads) => {
                    let label = account.label.clone().unwrap_or_else(|| account.id.clone());
                    query_accounts.insert(query.clone(), label);
                    collected.append(&mut ads);
                    state.store.touch_account_used(&account.id, now).await;
                    break;
                }
                Err(err) => {
                    if let Some(secs) = flood_wait_secs(&err) {
                        let until = now + Duration::seconds(secs);
                        let _ = state.store.set_account_flood(&account.id, until).await;
                        let label = account.label.clone().unwrap_or_else(|| account.id.clone());
                        let mut log = PanelLog::new(
                            "warning",
                            "Limit (FLOOD_WAIT)",
                            format!(
                                "{label} akkaunt {secs}s limitga tushdi. Keyingi akkauntga o'tildi."
                            ),
                        );
                        log.keyword = Some(query.clone());
                        log.source_channel = Some(label);
                        log.raw_response = Some(format!("{secs} soniya dam oladi (FLOOD_WAIT)"));
                        scan_logs.push(log);
                        continue; // keyingi akkaunt bilan shu keyni qayta sinaymiz
                    }

                    let message = format!("{query}: {err}");
                    state.runtime.write().await.last_error = Some(message.clone());
                    let mut log = PanelLog::new(
                        "error",
                        "Qidiruvda xato",
                        format!("'{query}' key bo'yicha qidiruvda xato."),
                    );
                    log.keyword = Some(query.clone());
                    log.source_channel =
                        Some(account.label.clone().unwrap_or_else(|| account.id.clone()));
                    log.raw_response = Some(err.to_string());
                    scan_logs.push(log);
                    break; // limit emas — bu keyni o'tkazamiz
                }
            }
        }
    }

    // Natijalarni saqlash xato bersa ham, shu paytgacha to'plangan loglar
    // (kanal xatolari) yo'qolmasligi uchun ularni avval flush qilamiz.
    let (added_items, seen_counts) = match state.store.push_results(collected.clone()).await {
        Ok(items) => items,
        Err(err) => {
            scan_logs.push(PanelLog::new(
                "error",
                "Natijalarni saqlashda xato",
                err.to_string(),
            ));
            let _ = state.store.push_logs(scan_logs).await;
            return Err(err);
        }
    };

    // 24 soatlik statistikaga har bir topilgan (kalit so'z, kanal) uchrashuvini yozamiz.
    let appearances: Vec<(String, String, Option<String>)> = collected
        .iter()
        .flat_map(|ad| {
            let channel = ad.channel.clone();
            let title = ad.channel_title.clone();
            ad.matched_keywords
                .iter()
                .map(move |kw| (kw.clone(), channel.clone(), title.clone()))
        })
        .collect();
    if let Err(err) = state.store.record_appearances(&appearances, now).await {
        scan_logs.push(PanelLog::new(
            "error",
            "Statistika saqlashda xato",
            err.to_string(),
        ));
    }

    let (action_logs, statuses) =
        process_scan_actions(state, &settings, &collected, &added_items, now).await;
    scan_logs.extend(action_logs);

    if let Err(err) = state.store.mark_keywords_checked(&keywords, now).await {
        scan_logs.push(PanelLog::new(
            "error",
            "Key holatini saqlashda xato",
            err.to_string(),
        ));
    }

    // Yakuniy log: har bir key bo'yicha kim qidirgani va nima chiqqani — har bir
    // reklama uchun aniq status (oq ro'yxat / order yuborilgan / limit kutilmoqda /
    // xato) va necha marta chiqqani.
    let mut detail_lines: Vec<String> = Vec::new();
    for query in &keywords {
        let account_label = query_accounts
            .get(query)
            .map(|label| label.as_str())
            .unwrap_or("—");
        let ads_for_query: Vec<&AdResult> = collected
            .iter()
            .filter(|ad| ad.matched_keywords.iter().any(|k| k == query))
            .collect();
        if ads_for_query.is_empty() {
            detail_lines.push(format!(
                "「{query}」 (qidirgan: {account_label}): hech narsa chiqmadi"
            ));
            continue;
        }
        detail_lines.push(format!(
            "「{query}」 (qidirgan: {account_label}): {} ta reklama",
            ads_for_query.len()
        ));
        for ad in ads_for_query {
            let count = seen_counts.get(&ad.fingerprint).copied().unwrap_or(1);
            let status = statuses
                .get(&ad.fingerprint)
                .map(|s| s.as_str())
                .unwrap_or("holat aniqlanmadi");
            detail_lines.push(format!(
                "  • @{} ({}) — {status}. Jami {count}-marta chiqishi.",
                ad.channel,
                if ad.title.is_empty() { "-" } else { &ad.title }
            ));
        }
    }

    let mut end_log = PanelLog::new(
        "success",
        "Scan yakunlandi",
        format!(
            "{} ta key bo'yicha qidirildi. Telegram {} ta reklama qaytardi ({} ta yangi).",
            keywords.len(),
            collected.len(),
            added_items.len()
        ),
    );
    end_log.keyword = Some(keywords.join(", "));
    end_log.source_channel = Some(
        query_accounts
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .join(", "),
    );
    end_log.raw_response = Some(detail_lines.join("\n"));
    scan_logs.push(end_log);
    state.store.push_logs(scan_logs).await?;

    Ok(ScanResponse {
        added: added_items.len(),
        checked_channels: 0,
        checked_keywords: keywords.len(),
        message: format!(
            "{} ta key bo'yicha qidirildi, {added} ta yangi natija",
            keywords.len(),
            added = added_items.len()
        ),
    })
}

/// Har bir topilgan reklama uchun order qarorini beradi. Qaytaradi:
/// (panel loglari, fingerprint → yakuniy status matni).
async fn process_scan_actions(
    state: &AppState,
    settings: &Settings,
    collected: &[AdResult],
    added: &[AdResult],
    now: DateTime<Utc>,
) -> (Vec<PanelLog>, HashMap<String, String>) {
    let mut logs = Vec::new();
    let mut statuses: HashMap<String, String> = HashMap::new();
    let added_fps: HashSet<&str> = added.iter().map(|ad| ad.fingerprint.as_str()).collect();

    // Reklamalarni kalit so'z bo'yicha guruhlaymiz. Order QARORI kalit so'z darajasida:
    // shu key bo'yicha oq ro'yxatda BO'LMAGAN kamida bitta kanal topilsa va oxirgi
    // orderdan 1 daqiqa o'tgan bo'lsa — bitta order yuboriladi. "Bir marta" cheklovi yo'q:
    // kanal takror chiqsa ham har daqiqada order ketaveradi.
    let mut by_keyword: HashMap<String, Vec<&AdResult>> = HashMap::new();
    for ad in collected {
        for keyword in &ad.matched_keywords {
            by_keyword.entry(keyword.clone()).or_default().push(ad);
        }
    }

    for (keyword, ads) in by_keyword {
        let mut orderable: Vec<&AdResult> = Vec::new();

        for ad in &ads {
            let target = ad
                .target_channel
                .clone()
                .or_else(|| normalize_channel_ref(&ad.url));
            if let Some(matched) =
                find_list_match(target.as_deref(), &ad.url, &settings.whitelist_channels)
            {
                statuses.insert(
                    ad.fingerprint.clone(),
                    "OQ RO'YXAT kanali — order yuborilmaydi".to_string(),
                );
                // Oq ro'yxat logi faqat yangi (added) reklama uchun bir marta.
                if added_fps.contains(ad.fingerprint.as_str()) {
                    let mut log = base_ad_log(
                        "warning",
                        "Order yuborilmadi: oq ro'yxat",
                        format!("{} oq ro'yxatda bor. Order yuborilmaydi.", matched.display),
                        ad,
                        &keyword,
                        Some(&matched),
                    );
                    log.raw_response = Some("SMMMAIN chaqirilmadi, sabab: oq ro'yxat".to_string());
                    logs.push(log);
                }
            } else {
                orderable.push(ad);
            }
        }

        // Shu key bo'yicha order oladigan (oq ro'yxatda bo'lmagan) kanal yo'q.
        if orderable.is_empty() {
            continue;
        }

        let Some(order_key) = matched_order_keys(settings, std::slice::from_ref(&keyword))
            .into_iter()
            .find(|key| !key.text.trim().is_empty())
        else {
            for ad in &orderable {
                statuses.insert(ad.fingerprint.clone(), "key qoidasi topilmadi".to_string());
            }
            continue;
        };

        // QAT'IY QOIDA: shu kalit so'z bo'yicha oxirgi orderdan kamida 1 daqiqa o'tishi
        // shart (natijadan qat'i nazar). Slotni atomik band qilamiz.
        match state
            .store
            .reserve_keyword_order(&order_key.text, MIN_ORDER_GAP_SECS, now)
            .await
        {
            Ok(Some(remaining)) => {
                for ad in &orderable {
                    statuses.insert(
                        ad.fingerprint.clone(),
                        format!("1 daqiqa limiti — {remaining}s dan keyin order"),
                    );
                }
                continue;
            }
            Ok(None) => {} // ruxsat — order yuboramiz
            Err(err) => {
                for ad in &orderable {
                    statuses.insert(ad.fingerprint.clone(), format!("limit xato: {err}"));
                }
                continue;
            }
        }

        // Shu key uchun bitta order. Vakil kanal — birinchi order oladigan kanal.
        let lead = orderable[0];
        let channels_str = orderable
            .iter()
            .map(|ad| format!("@{}", ad.channel))
            .collect::<Vec<_>>()
            .join(", ");
        let matched = ChannelMatch {
            display: channels_str.clone(),
            order_link: format!("https://t.me/{}", lead.channel),
        };

        let mut log = base_ad_log(
            "info",
            "Order yuborilmoqda",
            format!(
                "「{}」 bo'yicha oq ro'yxatda bo'lmagan kanal(lar): {channels_str}. SMMMAIN service {}, link {}, quality {}.",
                keyword, order_key.service_id, order_key.text, order_key.quantity
            ),
            lead,
            &order_key.text,
            Some(&matched),
        );
        log.order_link = Some(order_key.text.clone());
        log.service_id = Some(order_key.service_id);
        log.quantity = Some(order_key.quantity);

        let ad_status = match state
            .smmmain
            .send_order(order_key.service_id, &order_key.text, order_key.quantity)
            .await
        {
            Ok(outcome) => {
                log.level = "success".to_string();
                log.title = "Order yuborildi".to_string();
                log.message = format!(
                    "「{}」 bo'yicha SMMMAIN order yuborildi ({channels_str}). Link: {}, quality: {}.",
                    keyword, order_key.text, order_key.quantity
                );
                log.order_id = outcome.order_id.clone();
                log.raw_response = Some(outcome.raw_response);
                let _ = state
                    .store
                    .upsert_order_record(OrderRecord {
                        link: order_key.text.clone(),
                        order_id: outcome.order_id,
                        service_id: order_key.service_id,
                        quantity: order_key.quantity,
                        status: Some("pending".to_string()),
                        created_at: now,
                        last_checked_at: Some(now),
                    })
                    .await;
                "order yuborildi".to_string()
            }
            Err(err) => {
                log.level = "error".to_string();
                log.title = "Order yuborishda xato".to_string();
                log.message = format!(
                    "「{}」 bo'yicha SMMMAIN order yuborilmadi. Link: {}. Xato: {err}.",
                    keyword, order_key.text
                );
                log.raw_response = Some(err.to_string());
                state.runtime.write().await.last_error = Some(log.message.clone());
                format!("ORDER XATO: {err}")
            }
        };

        for ad in &orderable {
            statuses.insert(ad.fingerprint.clone(), ad_status.clone());
        }
        logs.push(log);
    }

    (logs, statuses)
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

fn selected_order_keys(
    settings: &Settings,
    now: DateTime<Utc>,
    force: bool,
    only_keyword: Option<&str>,
) -> Vec<OrderKey> {
    if settings.keyword_rules.is_empty() {
        return settings
            .keywords
            .iter()
            .map(|keyword| keyword.trim().to_string())
            .filter(|keyword| !keyword.is_empty())
            .filter(|keyword| {
                only_keyword
                    .map(|only| keyword.eq_ignore_ascii_case(only.trim()))
                    .unwrap_or(true)
            })
            .map(|text| OrderKey {
                text,
                service_id: DEFAULT_SMMMAIN_SERVICE_ID,
                quantity: settings.order_quantity,
            })
            .collect();
    }

    // Bitta key qo'lda tekshirilsa — yoqilgan-o'chirilganiga va jadvaliga qaramaymiz.
    if let Some(only) = only_keyword {
        return settings
            .keyword_rules
            .iter()
            .filter(|rule| rule.text.trim().eq_ignore_ascii_case(only.trim()))
            .filter_map(order_key_from_rule)
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
        service_id: DEFAULT_SMMMAIN_SERVICE_ID,
        quantity: rule.order_quantity.max(1),
    })
}
