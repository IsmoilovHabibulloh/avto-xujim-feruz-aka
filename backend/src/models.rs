use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const DEFAULT_SMMMAIN_SERVICE_ID: u64 = 39;

/// Bitta kalit so'z bo'yicha ketma-ket buyurtmalar orasidagi eng kam vaqt (sekund).
/// Qat'iy qoida: natija (muvaffaqiyat/xato) qanday bo'lishidan qat'i nazar, bir key
/// bo'yicha 1 daqiqada ko'pi bilan 1 marta order yuboriladi.
pub const MIN_ORDER_GAP_SECS: i64 = 60;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_interval_seconds")]
    pub interval_seconds: u64,
    #[serde(default)]
    pub keywords: Vec<String>,
    #[serde(default)]
    pub keyword_rules: Vec<KeywordRule>,
    #[serde(default)]
    pub channels: Vec<String>,
    #[serde(default)]
    pub whitelist_channels: Vec<String>,
    #[serde(default = "default_order_quantity")]
    pub order_quantity: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            interval_seconds: default_interval_seconds(),
            keywords: Vec::new(),
            keyword_rules: Vec::new(),
            channels: Vec::new(),
            whitelist_channels: Vec::new(),
            order_quantity: default_order_quantity(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KeywordRule {
    #[serde(default)]
    pub text: String,
    #[serde(default = "default_interval_seconds")]
    pub interval_seconds: u64,
    #[serde(default = "default_order_quantity")]
    pub order_quantity: u64,
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
            order_quantity: default_order_quantity(),
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
    /// Har bir reklama (fingerprint) jami necha marta qaytarilgani.
    #[serde(default)]
    pub seen_counts: HashMap<String, u64>,
    /// Order yuborib bo'lingan reklamalar (fingerprint). Oq ro'yxatda bo'lmagan
    /// reklama shu to'plamda bo'lmasa — order oladi (qachon topilganidan qat'i nazar).
    #[serde(default)]
    pub ordered: HashSet<String>,
    /// Har bir kalit so'z bo'yicha oxirgi order yuborilgan vaqt (1 daqiqalik limit uchun).
    #[serde(default)]
    pub last_order_at: HashMap<String, DateTime<Utc>>,
    /// So'nggi 24 soat statistikasi: kalit so'z -> kanal -> soatlik hisoblagichlar.
    #[serde(default)]
    pub stats: HashMap<String, HashMap<String, ChannelBuckets>>,
    #[serde(default)]
    pub logs: Vec<PanelLog>,
    #[serde(default)]
    pub orders: HashMap<String, OrderRecord>,
    #[serde(default)]
    pub accounts: Vec<TelegramAccount>,
}

impl Default for PersistedState {
    fn default() -> Self {
        Self {
            settings: Settings::default(),
            telegram: TelegramSettings::default(),
            results: Vec::new(),
            seen: HashSet::new(),
            seen_counts: HashMap::new(),
            ordered: HashSet::new(),
            last_order_at: HashMap::new(),
            stats: HashMap::new(),
            logs: Vec::new(),
            orders: HashMap::new(),
            accounts: Vec::new(),
        }
    }
}

/// Ulangan Telegram userbot akkaunti (QR orqali qo'shilgan).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TelegramAccount {
    pub id: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub first_name: Option<String>,
    #[serde(default)]
    pub last_name: Option<String>,
    #[serde(default)]
    pub phone: Option<String>,
    #[serde(default)]
    pub telegram_id: Option<i64>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub last_used_at: Option<DateTime<Utc>>,
    /// Shu vaqtgacha akkaunt FLOOD_WAIT sababli "dam oladi" (so'rov yuborilmaydi).
    #[serde(default)]
    pub flood_until: Option<DateTime<Utc>>,
}

/// Har bir order linki (key matni) uchun oxirgi yuborilgan order holati.
/// Bir xil reklama qayta topilganda qayta order yuborish-yubormaslikni hal qilish uchun ishlatiladi.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OrderRecord {
    pub link: String,
    #[serde(default)]
    pub order_id: Option<String>,
    pub service_id: u64,
    pub quantity: u64,
    #[serde(default)]
    pub status: Option<String>,
    pub created_at: DateTime<Utc>,
    #[serde(default)]
    pub last_checked_at: Option<DateTime<Utc>>,
}

/// Bitta kanal uchun soatlik uchrashuv hisoblagichlari (24 soatlik statistika uchun).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ChannelBuckets {
    #[serde(default)]
    pub title: Option<String>,
    /// Absolyut soat (unix_ts / 3600) -> shu soatda necha marta uchraganini.
    #[serde(default)]
    pub hourly: HashMap<i64, u64>,
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

#[derive(Clone, Debug, Serialize)]
pub struct SmmBalance {
    pub configured: bool,
    pub balance: Option<String>,
    pub currency: Option<String>,
    pub error: Option<String>,
    pub checked_at: DateTime<Utc>,
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
    pub smm_balance: SmmBalance,
    pub status: RuntimeStatus,
    pub results: Vec<AdResult>,
    pub logs: Vec<PanelLog>,
    pub accounts: Vec<AccountStatus>,
    /// So'nggi 24 soat statistikasi — har bir kalit so'z bo'yicha kanallar ulushi.
    pub stats_24h: Vec<KeywordStat>,
}

/// Bitta kalit so'z bo'yicha so'nggi 24 soatdagi kanallar taqsimoti (donut uchun).
#[derive(Clone, Debug, Serialize)]
pub struct KeywordStat {
    pub keyword: String,
    pub total: u64,
    pub whitelist_percent: f64,
    pub order_percent: f64,
    pub segments: Vec<ChannelSegment>,
}

/// Donut bo'lagi: bitta kanal, ulushi (foiz) va rangi (oq ro'yxatmi).
#[derive(Clone, Debug, Serialize)]
pub struct ChannelSegment {
    pub channel: String,
    pub title: Option<String>,
    pub whitelisted: bool,
    pub count: u64,
    pub percent: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AccountStatus {
    pub id: String,
    pub label: Option<String>,
    pub username: Option<String>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub phone: Option<String>,
    pub telegram_id: Option<i64>,
    pub connected: bool,
    pub flooded: bool,
    pub flood_until: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    pub last_used_at: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct CredentialsRequest {
    pub api_id: i32,
    pub api_hash: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct QrStartResponse {
    pub account_id: String,
    pub qr_url: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct AccountIdRequest {
    pub account_id: String,
}

/// Qo'lda scan so'rovi: `keyword` berilsa faqat o'sha key tekshiriladi.
#[derive(Clone, Debug, Deserialize)]
pub struct ScanRunRequest {
    #[serde(default)]
    pub keyword: Option<String>,
}

/// Qo'lda bitta order yuborish so'rovi (hech qanday tekshiruvsiz, darhol).
#[derive(Clone, Debug, Deserialize)]
pub struct OrderSendRequest {
    pub keyword: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct QrPasswordRequest {
    pub account_id: String,
    pub password: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct QrPollResponse {
    pub account_id: String,
    /// "waiting" | "password" | "connected" | "error"
    pub status: String,
    pub qr_url: Option<String>,
    pub expires_at: Option<DateTime<Utc>>,
    pub message: String,
}
