use crate::models::{
    AccountIdRequest, AccountStatus, CredentialsRequest, DashboardResponse, ErrorResponse,
    LoginRequest, LoginResponse, MeResponse, OrderSendRequest, PanelLog, QrPasswordRequest,
    QrPollResponse, QrStartResponse, RuntimeStatus, ScanRunRequest, Settings, SmmBalance,
    TelegramAccount, TelegramSettings,
};
use crate::scanner;
use crate::smmmain::SmmMainService;
use crate::store::Store;
use crate::telegram::QrOutcome;
use crate::telegram::TelegramService;
use axum::extract::State;
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use chrono::{DateTime, Utc};
use serde::Serialize;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub telegram: Arc<TelegramService>,
    pub smmmain: Arc<SmmMainService>,
    pub sessions: Arc<RwLock<HashMap<String, String>>>,
    pub runtime: Arc<RwLock<RuntimeInfo>>,
    /// Akkauntlar bo'yicha round-robin hisoblagich.
    pub rr: Arc<AtomicUsize>,
    pub admin_username: String,
    pub admin_password: String,
}

#[derive(Clone, Debug, Default)]
pub struct RuntimeInfo {
    pub scanning: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
}

impl AppState {
    pub async fn status(&self) -> RuntimeStatus {
        let runtime = self.runtime.read().await.clone();
        let snapshot = self.store.snapshot().await;
        let next_run_at = next_keyword_run_at(&snapshot.settings).or(runtime.next_run_at);
        let mut telegram_connected = false;
        for account in &snapshot.accounts {
            if self.telegram.is_account_connected(&account.id).await {
                telegram_connected = true;
                break;
            }
        }

        RuntimeStatus {
            telegram_connected,
            login_waiting_for: None,
            scanning: runtime.scanning,
            last_run_at: runtime.last_run_at,
            next_run_at,
            last_error: runtime.last_error,
            total_results: snapshot.results.len(),
            total_logs: snapshot.logs.len(),
        }
    }
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    pub fn unauthorized() -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: "Avtorizatsiya kerak".to_string(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }
}

impl<E> From<E> for ApiError
where
    E: Into<anyhow::Error>,
{
    fn from(value: E) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: value.into().to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorResponse {
                error: self.message,
            }),
        )
            .into_response()
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/auth/login", post(login))
        .route("/auth/logout", post(logout))
        .route("/me", get(me))
        .route("/dashboard", get(dashboard))
        .route("/settings", get(get_settings).put(update_settings))
        .route("/results", get(get_results).delete(clear_results))
        .route("/logs", get(get_logs).delete(clear_logs))
        .route("/smmmain/balance", get(smmmain_balance))
        .route("/status", get(status))
        .route("/scan/run", post(run_scan))
        .route("/order/send", post(send_manual_order))
        .route("/telegram/credentials", post(telegram_credentials))
        .route("/telegram/accounts", get(telegram_accounts))
        .route("/telegram/qr/start", post(telegram_qr_start))
        .route("/telegram/qr/poll", post(telegram_qr_poll))
        .route("/telegram/qr/password", post(telegram_qr_password))
        .route("/telegram/account/remove", post(telegram_account_remove))
        .with_state(state)
}

async fn login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, ApiError> {
    if payload.username == state.admin_username && payload.password == state.admin_password {
        let token = Uuid::new_v4().to_string();
        state
            .sessions
            .write()
            .await
            .insert(token.clone(), payload.username);
        Ok(Json(LoginResponse { token }))
    } else {
        Err(ApiError::unauthorized())
    }
}

async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SimpleMessage>, ApiError> {
    if let Some(token) = bearer_token(&headers) {
        state.sessions.write().await.remove(&token);
    }
    Ok(Json(SimpleMessage::new("Chiqildi")))
}

async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<MeResponse>, ApiError> {
    let username = require_auth(&headers, &state).await?;
    Ok(Json(MeResponse { username }))
}

async fn dashboard(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<DashboardResponse>, ApiError> {
    require_auth(&headers, &state).await?;
    let snapshot = state.store.snapshot().await;
    let accounts = account_statuses(&state, &snapshot.accounts).await;
    Ok(Json(DashboardResponse {
        settings: snapshot.settings,
        telegram: public_telegram_settings(snapshot.telegram),
        smm_balance: public_smm_balance(&state).await,
        status: state.status().await,
        results: snapshot.results,
        logs: snapshot.logs,
        accounts,
    }))
}

async fn account_statuses(state: &AppState, accounts: &[TelegramAccount]) -> Vec<AccountStatus> {
    let now = Utc::now();
    let mut out = Vec::with_capacity(accounts.len());
    for account in accounts {
        let connected = state.telegram.is_account_connected(&account.id).await;
        let flooded = account
            .flood_until
            .map(|until| until > now)
            .unwrap_or(false);
        let mut account = account.clone();
        // Eski akkauntlarda profil ma'lumoti bo'lmasligi mumkin — ulanganda to'ldirib qo'yamiz.
        if connected && account.telegram_id.is_none() {
            if let Some(me) = state.telegram.account_me(&account.id).await {
                if state
                    .store
                    .update_account_profile(&account.id, &me)
                    .await
                    .is_ok()
                {
                    account.username = me.username;
                    account.first_name = me.first_name;
                    account.last_name = me.last_name;
                    account.phone = me.phone;
                    account.telegram_id = me.telegram_id;
                }
            }
        }
        out.push(AccountStatus {
            id: account.id.clone(),
            label: account.label.clone(),
            username: account.username.clone(),
            first_name: account.first_name.clone(),
            last_name: account.last_name.clone(),
            phone: account.phone.clone(),
            telegram_id: account.telegram_id,
            connected,
            flooded,
            flood_until: account.flood_until,
            created_at: account.created_at,
            last_used_at: account.last_used_at,
        });
    }
    out
}

async fn smmmain_balance(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SmmBalance>, ApiError> {
    require_auth(&headers, &state).await?;
    Ok(Json(public_smm_balance(&state).await))
}

async fn get_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Settings>, ApiError> {
    require_auth(&headers, &state).await?;
    Ok(Json(state.store.settings().await))
}

async fn update_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(settings): Json<Settings>,
) -> Result<Json<Settings>, ApiError> {
    require_auth(&headers, &state).await?;
    let clean = state.store.update_settings(settings).await?;
    Ok(Json(clean))
}

async fn get_results(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<crate::models::AdResult>>, ApiError> {
    require_auth(&headers, &state).await?;
    Ok(Json(state.store.snapshot().await.results))
}

async fn clear_results(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SimpleMessage>, ApiError> {
    require_auth(&headers, &state).await?;
    state.store.clear_results().await?;
    Ok(Json(SimpleMessage::new("Natijalar tozalandi")))
}

async fn get_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<PanelLog>>, ApiError> {
    require_auth(&headers, &state).await?;
    Ok(Json(state.store.snapshot().await.logs))
}

async fn clear_logs(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SimpleMessage>, ApiError> {
    require_auth(&headers, &state).await?;
    state.store.clear_logs().await?;
    Ok(Json(SimpleMessage::new("Loglar tozalandi")))
}

async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<RuntimeStatus>, ApiError> {
    require_auth(&headers, &state).await?;
    Ok(Json(state.status().await))
}

async fn run_scan(
    State(state): State<AppState>,
    headers: HeaderMap,
    payload: Option<Json<ScanRunRequest>>,
) -> Result<Json<crate::models::ScanResponse>, ApiError> {
    require_auth(&headers, &state).await?;
    let keyword = payload
        .and_then(|Json(body)| body.keyword)
        .map(|keyword| keyword.trim().to_string())
        .filter(|keyword| !keyword.is_empty());
    Ok(Json(scanner::scan_once(state, keyword).await?))
}

/// Qo'lda bitta order yuboradi: hech qanday oq ro'yxat/holat tekshiruvisiz,
/// bosgan zahoti keyning belgilangan miqdori bilan SMMMAIN'ga ketadi.
async fn send_manual_order(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<OrderSendRequest>,
) -> Result<Json<SimpleMessage>, ApiError> {
    require_auth(&headers, &state).await?;
    let keyword = payload.keyword.trim().to_string();
    if keyword.is_empty() {
        return Err(ApiError::bad_request("Key bo'sh"));
    }

    let settings = state.store.settings().await;
    let rule = settings
        .keyword_rules
        .iter()
        .find(|rule| rule.text.trim().eq_ignore_ascii_case(&keyword))
        .cloned()
        .ok_or_else(|| ApiError::bad_request("Bunday key topilmadi"))?;

    let quantity = rule.order_quantity.max(1);

    // QAT'IY QOIDA: shu kalit so'z bo'yicha oxirgi orderdan kamida 1 daqiqa o'tgan
    // bo'lishi shart (qo'lda ham, avtomatik ham — bir xil limit).
    let now = Utc::now();
    if let Some(remaining) = state
        .store
        .reserve_keyword_order(&rule.text, crate::models::MIN_ORDER_GAP_SECS, now)
        .await?
    {
        return Err(ApiError::bad_request(format!(
            "Kalit so'z limiti: \"{}\" uchun {remaining} sekunddan keyin order yuboriladi (1 daqiqada 1 marta).",
            rule.text
        )));
    }

    let mut log = PanelLog::new("info", "Qo'lda order", String::new());
    log.keyword = Some(rule.text.clone());
    log.source_channel = Some("Admin panel".to_string());
    log.order_link = Some(rule.text.clone());
    log.quantity = Some(quantity);

    match state
        .smmmain
        .send_order(
            crate::models::DEFAULT_SMMMAIN_SERVICE_ID,
            &rule.text,
            quantity,
        )
        .await
    {
        Ok(outcome) => {
            let _ = state
                .store
                .upsert_order_record(crate::models::OrderRecord {
                    link: rule.text.clone(),
                    order_id: outcome.order_id.clone(),
                    service_id: crate::models::DEFAULT_SMMMAIN_SERVICE_ID,
                    quantity,
                    status: Some("pending".to_string()),
                    created_at: now,
                    last_checked_at: Some(now),
                })
                .await;
            log.level = "success".to_string();
            log.title = "Qo'lda order yuborildi".to_string();
            log.message = format!(
                "\"{}\" uchun admin tomonidan order yuborildi. Quality: {quantity}.",
                rule.text
            );
            log.order_id = outcome.order_id.clone();
            log.raw_response = Some(outcome.raw_response);
            let _ = state.store.push_logs(vec![log]).await;
            Ok(Json(SimpleMessage::new(format!(
                "Order yuborildi: \"{}\" (quality {quantity})",
                rule.text
            ))))
        }
        Err(err) => {
            log.level = "error".to_string();
            log.title = "Qo'lda order xato".to_string();
            log.message = format!("\"{}\" uchun order yuborilmadi: {err}", rule.text);
            log.raw_response = Some(err.to_string());
            let _ = state.store.push_logs(vec![log]).await;
            Err(ApiError::bad_request(format!("Order xato: {err}")))
        }
    }
}

async fn telegram_credentials(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CredentialsRequest>,
) -> Result<Json<SimpleMessage>, ApiError> {
    require_auth(&headers, &state).await?;
    if payload.api_id <= 0 || payload.api_hash.trim().is_empty() {
        return Err(ApiError::bad_request("API ID va API hash kerak"));
    }
    let mut settings = state.store.telegram_settings().await;
    settings.api_id = Some(payload.api_id);
    settings.api_hash = Some(payload.api_hash.trim().to_string());
    state.store.update_telegram(settings).await?;
    Ok(Json(SimpleMessage::new("API ma'lumotlari saqlandi")))
}

async fn telegram_accounts(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Vec<AccountStatus>>, ApiError> {
    require_auth(&headers, &state).await?;
    let accounts = state.store.accounts().await;
    Ok(Json(account_statuses(&state, &accounts).await))
}

async fn telegram_qr_start(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<QrStartResponse>, ApiError> {
    require_auth(&headers, &state).await?;
    let settings = state.store.telegram_settings().await;
    let api_id = settings
        .api_id
        .ok_or_else(|| ApiError::bad_request("Avval API ID/hash kiriting"))?;
    let api_hash = settings
        .api_hash
        .filter(|h| !h.trim().is_empty())
        .ok_or_else(|| ApiError::bad_request("Avval API ID/hash kiriting"))?;

    let account_id = Uuid::new_v4().to_string();
    let (qr_url, expires_at) = state
        .telegram
        .start_qr(&account_id, api_id, &api_hash)
        .await?;

    Ok(Json(QrStartResponse {
        account_id,
        qr_url,
        expires_at,
    }))
}

async fn telegram_qr_poll(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<AccountIdRequest>,
) -> Result<Json<QrPollResponse>, ApiError> {
    require_auth(&headers, &state).await?;
    let outcome = state.telegram.poll_qr(&payload.account_id).await?;
    Ok(Json(
        qr_response(&state, &payload.account_id, outcome).await?,
    ))
}

async fn telegram_qr_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<QrPasswordRequest>,
) -> Result<Json<QrPollResponse>, ApiError> {
    require_auth(&headers, &state).await?;
    if payload.password.trim().is_empty() {
        return Err(ApiError::bad_request("2FA parol kerak"));
    }
    let outcome = state
        .telegram
        .submit_qr_password(&payload.account_id, payload.password.trim())
        .await?;
    Ok(Json(
        qr_response(&state, &payload.account_id, outcome).await?,
    ))
}

async fn telegram_account_remove(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<AccountIdRequest>,
) -> Result<Json<SimpleMessage>, ApiError> {
    require_auth(&headers, &state).await?;
    state
        .telegram
        .remove_account_session(&payload.account_id)
        .await?;
    state.store.remove_account(&payload.account_id).await?;
    Ok(Json(SimpleMessage::new("Akkaunt o'chirildi")))
}

/// QR natijasini javobga aylantiradi; ulanganda akkauntni saqlaydi.
async fn qr_response(
    state: &AppState,
    account_id: &str,
    outcome: QrOutcome,
) -> Result<QrPollResponse, ApiError> {
    match outcome {
        QrOutcome::Waiting { qr_url, expires_at } => Ok(QrPollResponse {
            account_id: account_id.to_string(),
            status: "waiting".to_string(),
            qr_url: Some(qr_url),
            expires_at: Some(expires_at),
            message: "QR kutilmoqda".to_string(),
        }),
        QrOutcome::NeedPassword => Ok(QrPollResponse {
            account_id: account_id.to_string(),
            status: "password".to_string(),
            qr_url: None,
            expires_at: None,
            message: "2FA parol kerak".to_string(),
        }),
        QrOutcome::Connected { me } => {
            // Akkaunt allaqachon ro'yxatda bo'lmasa, qo'shamiz; bo'lsa profilini yangilaymiz.
            let exists = state
                .store
                .accounts()
                .await
                .iter()
                .any(|a| a.id == account_id);
            if exists {
                state.store.update_account_profile(account_id, &me).await?;
            } else {
                let full_name = [me.first_name.as_deref(), me.last_name.as_deref()]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                let label = if !full_name.is_empty() {
                    full_name
                } else if let Some(u) = &me.username {
                    format!("@{u}")
                } else {
                    "Akkaunt".to_string()
                };
                state
                    .store
                    .add_account(TelegramAccount {
                        id: account_id.to_string(),
                        label: Some(label),
                        username: me.username.clone(),
                        first_name: me.first_name.clone(),
                        last_name: me.last_name.clone(),
                        phone: me.phone.clone(),
                        telegram_id: me.telegram_id,
                        created_at: Utc::now(),
                        last_used_at: None,
                        flood_until: None,
                    })
                    .await?;
            }
            Ok(QrPollResponse {
                account_id: account_id.to_string(),
                status: "connected".to_string(),
                qr_url: None,
                expires_at: None,
                message: "Akkaunt ulandi".to_string(),
            })
        }
    }
}

fn bearer_token(headers: &HeaderMap) -> Option<String> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(|value| value.to_string())
}

async fn require_auth(headers: &HeaderMap, state: &AppState) -> Result<String, ApiError> {
    let token = bearer_token(headers).ok_or_else(ApiError::unauthorized)?;

    state
        .sessions
        .read()
        .await
        .get(&token)
        .cloned()
        .ok_or_else(ApiError::unauthorized)
}

fn public_telegram_settings(mut settings: TelegramSettings) -> TelegramSettings {
    if settings.api_hash.is_some() {
        settings.api_hash = Some("configured".to_string());
    }
    settings
}

async fn public_smm_balance(state: &AppState) -> SmmBalance {
    let checked_at = Utc::now();
    if !state.smmmain.is_configured() {
        return SmmBalance {
            configured: false,
            balance: None,
            currency: None,
            error: Some("SMMMAIN_API_KEY kiritilmagan".to_string()),
            checked_at,
        };
    }

    match state.smmmain.balance().await {
        Ok(outcome) => SmmBalance {
            configured: true,
            balance: outcome.balance,
            currency: outcome.currency,
            error: None,
            checked_at,
        },
        Err(err) => SmmBalance {
            configured: true,
            balance: None,
            currency: None,
            error: Some(err.to_string()),
            checked_at,
        },
    }
}

fn next_keyword_run_at(settings: &Settings) -> Option<DateTime<Utc>> {
    settings
        .keyword_rules
        .iter()
        .filter(|rule| rule.enabled)
        .filter_map(|rule| rule.next_check_at)
        .min()
}

#[derive(Serialize)]
struct SimpleMessage {
    message: String,
}

impl SimpleMessage {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}
