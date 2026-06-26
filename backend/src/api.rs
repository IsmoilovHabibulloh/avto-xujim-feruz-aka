use crate::models::{
    DashboardResponse, ErrorResponse, LoginRequest, LoginResponse, MeResponse, PanelLog,
    RuntimeStatus, Settings, TelegramRequestCode, TelegramSettings, TelegramVerifyCode,
};
use crate::scanner;
use crate::smmmain::SmmMainService;
use crate::store::Store;
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
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Clone)]
pub struct AppState {
    pub store: Arc<Store>,
    pub telegram: Arc<TelegramService>,
    pub smmmain: Arc<SmmMainService>,
    pub sessions: Arc<RwLock<HashMap<String, String>>>,
    pub runtime: Arc<RwLock<RuntimeInfo>>,
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

        RuntimeStatus {
            telegram_connected: self.telegram.is_connected().await,
            login_waiting_for: self.telegram.waiting_for().await,
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
        .route("/me", get(me))
        .route("/dashboard", get(dashboard))
        .route("/settings", get(get_settings).put(update_settings))
        .route("/results", get(get_results).delete(clear_results))
        .route("/logs", get(get_logs).delete(clear_logs))
        .route("/status", get(status))
        .route("/scan/run", post(run_scan))
        .route("/telegram/request-code", post(telegram_request_code))
        .route("/telegram/verify-code", post(telegram_verify_code))
        .route("/telegram/disconnect", post(telegram_disconnect))
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
    Ok(Json(DashboardResponse {
        settings: snapshot.settings,
        telegram: public_telegram_settings(snapshot.telegram),
        status: state.status().await,
        results: snapshot.results,
        logs: snapshot.logs,
    }))
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
    Ok(Json(state.store.update_settings(settings).await?))
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
) -> Result<Json<crate::models::ScanResponse>, ApiError> {
    require_auth(&headers, &state).await?;
    Ok(Json(scanner::scan_once(state).await?))
}

async fn telegram_request_code(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<TelegramRequestCode>,
) -> Result<Json<crate::models::TelegramAuthResponse>, ApiError> {
    require_auth(&headers, &state).await?;

    if payload.api_id <= 0 || payload.api_hash.trim().is_empty() || payload.phone.trim().is_empty()
    {
        return Err(ApiError::bad_request("API ID, API hash va telefon kerak"));
    }

    let settings = TelegramSettings {
        api_id: Some(payload.api_id),
        api_hash: Some(payload.api_hash.trim().to_string()),
        phone: Some(payload.phone.trim().to_string()),
    };
    state.store.update_telegram(settings.clone()).await?;

    Ok(Json(
        state
            .telegram
            .request_code(
                payload.api_id,
                settings.api_hash.unwrap_or_default(),
                settings.phone.unwrap_or_default(),
            )
            .await?,
    ))
}

async fn telegram_verify_code(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<TelegramVerifyCode>,
) -> Result<Json<crate::models::TelegramAuthResponse>, ApiError> {
    require_auth(&headers, &state).await?;
    Ok(Json(
        state
            .telegram
            .verify_code(payload.code, payload.password)
            .await?,
    ))
}

async fn telegram_disconnect(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<SimpleMessage>, ApiError> {
    require_auth(&headers, &state).await?;
    state.telegram.disconnect().await;
    Ok(Json(SimpleMessage::new("Userbot uzildi")))
}

async fn require_auth(headers: &HeaderMap, state: &AppState) -> Result<String, ApiError> {
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or_else(ApiError::unauthorized)?;

    state
        .sessions
        .read()
        .await
        .get(token)
        .cloned()
        .ok_or_else(ApiError::unauthorized)
}

fn public_telegram_settings(mut settings: TelegramSettings) -> TelegramSettings {
    if settings.api_hash.is_some() {
        settings.api_hash = Some("configured".to_string());
    }
    settings
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
