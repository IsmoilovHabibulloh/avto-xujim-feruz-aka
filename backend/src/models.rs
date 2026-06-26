use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default = "default_interval_seconds")]
    pub interval_seconds: u64,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub keyword_rules: Vec<KeywordRule>,
    #[serde(default)]
    pub channels: Vec<String>,
    #[serde(default)]
    pub blacklist_channels: Vec<String>,
    #[serde(default)]
    pub whitelist_channels: Vec<String>,
    #[serde(default = "default_order_quantity")]
    pub order_quantity: u64,
    #[serde(default = "default_max_results")]
    pub max_results: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            interval_seconds: default_interval_seconds(),
            keywords: Vec::new(),
            keyword_rules: Vec::new(),
            channels: Vec::new(),
            blacklist_channels: Vec::new(),
            whitelist_channels: Vec::new(),
            order_quantity: default_order_quantity(),
            max_results: default_max_results(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeywordRule {
    pub text: String,
    #[serde(default = "default_interval_seconds")]
    pub interval_seconds: u64,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub last_checked_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub next_check_at: Option<DateTime<Utc>>,
}

impl KeywordRule {
    pub fn new(text: String, interval_seconds: u64) -> Self {
        Self {
            text,
            interval_seconds,
            enabled: true,
            last_checked_at: None,
            next_check_at: None,
        }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_interval_seconds() -> u64 {
    5
}

fn default_max_results() -> usize {
    500
}

fn default_order_quantity() -> u64 {
    100
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct TelegramSettings {
    pub api_id: Option<i32>,
    pub api_hash: Option<String>,
    pub phone: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdResult {
    pub id: String,
    pub fingerprint: String,
    pub channel: String,
    pub channel_title: Option<String>,
    #[serde(default)]
    pub target_channel: Option<String>,
    pub matched_keywords: Vec<String>,
    pub title: String,
    pub message: String,
    pub url: String,
    pub button_text: String,
    pub sponsor_info: Option<String>,
    pub additional_info: Option<String>,
    pub recommended: bool,
    pub random_id_hex: String,
    pub found_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PersistedState {
    #[serde(default)]
    pub settings: Settings,
    #[serde(default)]
    pub telegram: TelegramSettings,
    #[serde(default)]
    pub results: Vec<AdResult>,
    #[serde(default)]
    pub seen: HashSet<String>,
    #[serde(default)]
    pub logs: Vec<PanelLog>,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            telegram: TelegramSettings::default(),
            results: Vec::new(),
            seen: HashSet::new(),
            logs: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PanelLog {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub level: String,
    pub title: String,
    pub message: String,
    #[serde(default)]
    pub keyword: Option<String>,
    #[serde(default)]
    pub source_channel: Option<String>,
    #[serde(default)]
    pub target_channel: Option<String>,
    #[serde(default)]
    pub ad_url: Option<String>,
    #[serde(default)]
    pub order_link: Option<String>,
    #[serde(default)]
    pub quantity: Option<u64>,
    #[serde(default)]
    pub service_id: Option<u64>,
    #[serde(default)]
    pub order_id: Option<String>,
    #[serde(default)]
    pub raw_response: Option<String>,
}

impl PanelLog {
    pub fn new(
        level: impl Into<String>,
        title: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            created_at: Utc::now(),
            level: level.into(),
            title: title.into(),
            message: message.into(),
            keyword: None,
            source_channel: None,
            target_channel: None,
            ad_url: None,
            order_link: None,
            quantity: None,
            service_id: None,
            order_id: None,
            raw_response: None,
        }
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct RuntimeStatus {
    pub telegram_connected: bool,
    pub login_waiting_for: Option<String>,
    pub scanning: bool,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub total_results: usize,
    pub total_logs: usize,
}

#[derive(Clone, Debug, Deserialize)]
pub struct LoginRequest {
    pub username: String,
    pub password: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct LoginResponse {
    pub token: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct MeResponse {
    pub username: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TelegramRequestCode {
    pub api_id: i32,
    pub api_hash: String,
    pub phone: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct TelegramVerifyCode {
    pub code: Option<String>,
    pub password: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct TelegramAuthResponse {
    pub connected: bool,
    pub waiting_for: Option<String>,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScanResponse {
    pub added: usize,
    pub checked_channels: usize,
    pub checked_keywords: usize,
    pub message: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct DashboardResponse {
    pub settings: Settings,
    pub telegram: TelegramSettings,
    pub status: RuntimeStatus,
    pub results: Vec<AdResult>,
    pub logs: Vec<PanelLog>,
}
