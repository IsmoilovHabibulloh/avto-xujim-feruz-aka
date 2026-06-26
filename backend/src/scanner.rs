use crate::api::AppState;
use crate::models::{
    AdResult, DEFAULT_SMMMAIN_SERVICE_ID, KeywordRule, OrderRecord, PanelLog, ScanResponse, Settings,
};
use crate::telegram::normalize_channel_ref;
use anyhow::{Result, anyhow};
use chrono::{DateTime, Duration, Utc};
use std::collections::HashSet;
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

/// Bir xil reklama qayta topilganda, oldingi order hali bajarilmagan bo'lsa
/// shuncha daqiqa kutib turamiz; shu vaqtdan keyin baribir qayta yuborishga ruxsat.
const ORDER_RECHECK_MINUTES: i64 = 10;

/// SMMMAIN `status` endpointini eng ko'pi bilan shu sekundda bir marta chaqiramiz.
/// Skan intervali kichik (masalan 5s) bo'lsa ham, bir order uchun status so'rovi
/// shu vaqtdan tez-tez yuborilmaydi — API rate-limitiga tushmaslik uchun.
const ORDER_STATUS_MIN_RECHECK_SECONDS: i64 = 30;

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

    // scan_inner'ni alohida taskда ishga tushiramiz: agar u panik bersa ham
    // `scanning` bayrog'i quyida albatta tiklanadi. Aks holda bitta panik skanerni
    // abadiy "ishlayapti" holatida qoldirib, undan keyingi barcha skanlarni bloklardi.
    let task_state = state.clone();
    let join = tokio::spawn(async move { scan_inner(&task_state, force).await }).await;

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

async fn scan_inner(state: &AppState, force: bool) -> Result<ScanResponse> {
    let settings = state.store.settings().await;
    let telegram_settings = state.store.telegram_settings().await;

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
    let mut scan_logs = vec![PanelLog::new(
        "info",
        "Scan boshlandi",
        format!(
            "{} ta key bo'yicha global qidiruv: {}. Akkauntlar: {}",
            keywords.len(),
            keywords.join(", "),
            accounts.len()
        ),
    )];

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

            let client = match state.telegram.ensure_account_client(&account.id, api_id).await {
                Ok(client) => client,
                Err(err) => {
                    let mut log = PanelLog::new(
                        "warning",
                        "Akkaunt ulanmadi",
                        format!(
                            "{} akkaunt ulanmadi: {err}",
                            account.label.clone().unwrap_or_else(|| account.id.clone())
                        ),
                    );
                    log.keyword = Some(query.clone());
                    scan_logs.push(log);
                    continue;
                }
            };

            match state.telegram.get_sponsored_peers(&client, query).await {
                Ok(mut ads) => {
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
                        scan_logs.push(log);
                        continue; // keyingi akkaunt bilan shu keyni qayta sinaymiz
                    }

                    let message = format!("{query}: {err}");
                    state.runtime.write().await.last_error = Some(message.clone());
                    let mut log = PanelLog::new(
                        "error",
                        "Qidiruvda xato",
                        format!("'{query}' key bo'yicha qidiruvda xato: {err}"),
                    );
                    log.keyword = Some(query.clone());
                    scan_logs.push(log);
                    break; // limit emas — bu keyni o'tkazamiz
                }
            }
        }
    }

    // Natijalarni saqlash xato bersa ham, shu paytgacha to'plangan loglar
    // (kanal xatolari) yo'qolmasligi uchun ularni avval flush qilamiz.
    let added_items = match state.store.push_results(collected.clone()).await {
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

    let action_logs = process_scan_actions(state, &settings, &collected, &added_items).await;
    scan_logs.extend(action_logs);

    if let Err(err) = state.store.mark_keywords_checked(&keywords, now).await {
        scan_logs.push(PanelLog::new(
            "error",
            "Key holatini saqlashda xato",
            err.to_string(),
        ));
    }

    scan_logs.push(PanelLog::new(
        "success",
        "Scan yakunlandi",
        format!(
            "{} ta key bo'yicha qidirildi. {} ta yangi natija topildi.",
            keywords.len(),
            added_items.len()
        ),
    ));
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

/// Order yuborish-yubormaslik qarori. String — sabab (log uchun).
enum OrderDecision {
    /// Order (qayta) yuborilsin.
    Place(String),
    /// Hozircha kutilsin.
    Wait(String),
}

async fn process_scan_actions(
    state: &AppState,
    settings: &Settings,
    collected: &[AdResult],
    added: &[AdResult],
) -> Vec<PanelLog> {
    let mut logs = Vec::new();
    let now = Utc::now();

    // 1) Yangi topilgan, lekin order chiqarmaydigan reklamalar uchun bir martalik
    //    info loglar (oq ro'yxat / ro'yxatda yo'q). Bular faqat `added` (birinchi
    //    marta ko'rilgan) reklamalar uchun yoziladi, shuning uchun har skanда takror
    //    bo'lmaydi.
    for ad in added {
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
                    "{} oq ro'yxatda bor. Key: {display_keywords}. Order yuborilmaydi.",
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

        if black_match.is_none() {
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
        }
    }

    // 2) Order gating — bu skanда topilgan barcha qora-ro'yxat mos reklamalar.
    //    Har bir link (key matni) bo'yicha bir marta hal qilinadi. `collected`
    //    (faqat `added` emas) ishlatiladi, chunki bir reklama qayta topilganda
    //    oldingi order holatiga qarab qayta yuborilishi mumkin.
    let mut handled_links: HashSet<String> = HashSet::new();

    for ad in collected {
        let target = ad
            .target_channel
            .clone()
            .or_else(|| normalize_channel_ref(&ad.url));
        let Some(matched) = find_list_match(target.as_deref(), &ad.url, &settings.blacklist_channels)
        else {
            continue;
        };
        // Oq ro'yxat qora ro'yxatdan ustun: oq ro'yxatda bo'lsa order yo'q.
        if find_list_match(target.as_deref(), &ad.url, &settings.whitelist_channels).is_some() {
            continue;
        }

        for order_key in matched_order_keys(settings, &ad.matched_keywords) {
            let link_key = order_key.text.trim().to_lowercase();
            if link_key.is_empty() || !handled_links.insert(link_key) {
                continue;
            }

            let record = state.store.order_record(&order_key.text).await;
            match decide_order(state, &record, now).await {
                OrderDecision::Wait(reason) => {
                    // Jimgina kutamiz — loglarni spam qilmaymiz. Holat yangilanishi
                    // (agar status tekshirilgan bo'lsa) decide_order ichida bo'ladi.
                    info!(link = %order_key.text, reason = %reason, "order kutilyapti");
                }
                OrderDecision::Place(reason) => {
                    let mut log = base_ad_log(
                        "info",
                        "Order yuborilmoqda",
                        format!(
                            "{} qora ro'yxatda. {reason}. SMMMAIN service {}, link {}, quality {}.",
                            matched.display, order_key.service_id, order_key.text, order_key.quantity
                        ),
                        ad,
                        &order_key.text,
                        Some(&matched),
                    );
                    log.order_link = Some(order_key.text.clone());
                    log.service_id = Some(order_key.service_id);
                    log.quantity = Some(order_key.quantity);

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
                                matched.display,
                                order_key.text,
                                order_key.service_id,
                                order_key.quantity
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

                            // Muvaffaqiyatsiz urinishni ham yozib qo'yamiz (order_id yo'q):
                            // shu link uchun 10 daqiqagacha qayta urinmaslik, ya'ni SMMMAIN'ni
                            // doimiy xatoda bombardimon qilmaslik uchun.
                            let _ = state
                                .store
                                .upsert_order_record(OrderRecord {
                                    link: order_key.text.clone(),
                                    order_id: None,
                                    service_id: order_key.service_id,
                                    quantity: order_key.quantity,
                                    status: Some("error".to_string()),
                                    created_at: now,
                                    last_checked_at: Some(now),
                                })
                                .await;
                        }
                    }

                    logs.push(log);
                }
            }
        }
    }

    logs
}

/// Berilgan link uchun order (qayta) yuborilishini hal qiladi.
///
/// - Yozuv yo'q bo'lsa → yuboriladi (birinchi marta).
/// - Oxirgi orderdan `ORDER_RECHECK_MINUTES` daqiqa o'tgan bo'lsa → qayta yuboriladi.
/// - Aks holda oldingi order holati tekshiriladi: bajarilgan bo'lsa → qayta yuboriladi,
///   hali bajarilayotgan/noma'lum bo'lsa → kutiladi.
async fn decide_order(
    state: &AppState,
    record: &Option<OrderRecord>,
    now: DateTime<Utc>,
) -> OrderDecision {
    let Some(record) = record else {
        return OrderDecision::Place("birinchi marta order yuborilmoqda".to_string());
    };

    let elapsed = (now - record.created_at).num_minutes();
    if elapsed >= ORDER_RECHECK_MINUTES {
        return OrderDecision::Place(format!(
            "oldingi orderdan {elapsed} daqiqa o'tdi, qayta yuboriladi"
        ));
    }

    let Some(order_id) = record.order_id.as_deref() else {
        return OrderDecision::Wait(
            "oldingi urinish muvaffaqiyatsiz edi, qayta urinishdan oldin kutilyapti".to_string(),
        );
    };

    // Status endpointini juda tez-tez chaqirmaymiz: oxirgi tekshiruvdan beri
    // ORDER_STATUS_MIN_RECHECK_SECONDS o'tmagan bo'lsa, eski holat bilan kutamiz.
    if let Some(last) = record.last_checked_at {
        if (now - last).num_seconds() < ORDER_STATUS_MIN_RECHECK_SECONDS {
            return OrderDecision::Wait(format!(
                "oldingi order yaqinda tekshirilgan (holat: {})",
                record.status.clone().unwrap_or_else(|| "noma'lum".to_string())
            ));
        }
    }

    match state.smmmain.order_status(order_id).await {
        Ok(outcome) => {
            let label = outcome
                .status
                .clone()
                .unwrap_or_else(|| "noma'lum".to_string());
            // Yangi holatni xotirada yangilaymiz (last_checked_at = now), shunda
            // throttle keyingi safar to'g'ri ishlaydi.
            state
                .store
                .touch_order_status(&record.link, outcome.status.clone(), now)
                .await;
            if is_order_active(outcome.status.as_deref()) {
                OrderDecision::Wait(format!("oldingi order hali bajarilmoqda (holat: {label})"))
            } else {
                OrderDecision::Place(format!("oldingi order yakunlandi (holat: {label})"))
            }
        }
        Err(err) => {
            // Tekshirish vaqtini yangilaymiz, shunda xato beruvchi endpointni
            // darhol qayta urinmaymiz.
            state
                .store
                .touch_order_status(&record.link, record.status.clone(), now)
                .await;
            OrderDecision::Wait(format!("oldingi order holatini olib bo'lmadi: {err}"))
        }
    }
}

/// Order holati hali "ishlayotgan" (yakunlanmagan) bo'lsa true.
/// Noma'lum (None) holat ham xavfsizlik uchun "ishlayapti" deb hisoblanadi,
/// shuning uchun faqat 10 daqiqa o'tgachgina qayta yuboriladi.
fn is_order_active(status: Option<&str>) -> bool {
    match status {
        Some(raw) => matches!(
            raw.trim().to_lowercase().as_str(),
            "pending"
                | "in progress"
                | "in_progress"
                | "inprogress"
                | "processing"
                | "queue"
                | "queued"
                | "active"
                | "started"
        ),
        None => true,
    }
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
