use anyhow::{Context, Result, anyhow, bail};
use grammers_client::{Client, SignInError};
use grammers_mtsender::SenderPool;
use grammers_session::storages::SqliteSession;
use grammers_tl_types as tl;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::models::{AdResult, TelegramAuthResponse, TelegramSettings};

pub struct TelegramService {
    session_path: PathBuf,
    active: Mutex<Option<ActiveClient>>,
    pending: Mutex<Option<PendingLogin>>,
}

struct ActiveClient {
    client: Client,
    runner: JoinHandle<()>,
}

enum PendingStep {
    Code(grammers_client::client::LoginToken),
    Password(grammers_client::client::PasswordToken),
}

struct PendingLogin {
    client: Client,
    runner: JoinHandle<()>,
    step: PendingStep,
}

impl TelegramService {
    pub fn new(session_path: impl AsRef<Path>) -> Self {
        Self {
            session_path: session_path.as_ref().to_path_buf(),
            active: Mutex::new(None),
            pending: Mutex::new(None),
        }
    }

    pub async fn is_connected(&self) -> bool {
        let active = self.active.lock().await;
        if let Some(active) = active.as_ref() {
            active.client.is_authorized().await.unwrap_or(false)
        } else {
            false
        }
    }

    pub async fn waiting_for(&self) -> Option<String> {
        let pending = self.pending.lock().await;
        pending.as_ref().map(|pending| match pending.step {
            PendingStep::Code(_) => "code".to_string(),
            PendingStep::Password(_) => "password".to_string(),
        })
    }

    pub async fn disconnect(&self) {
        if let Some(active) = self.active.lock().await.take() {
            active.runner.abort();
        }
        if let Some(pending) = self.pending.lock().await.take() {
            pending.runner.abort();
        }
    }

    pub async fn request_code(
        &self,
        api_id: i32,
        api_hash: String,
        phone: String,
    ) -> Result<TelegramAuthResponse> {
        let (client, runner) = self.connect(api_id).await?;

        if client.is_authorized().await? {
            self.set_active(client, runner).await;
            return Ok(TelegramAuthResponse {
                connected: true,
                waiting_for: None,
                message: "Userbot allaqachon ulangan".to_string(),
            });
        }

        let token = client
            .request_login_code(&phone, &api_hash)
            .await
            .context("Telegram login kodi so'rovida xatolik")?;

        if let Some(old) = self.pending.lock().await.replace(PendingLogin {
            client,
            runner,
            step: PendingStep::Code(token),
        }) {
            old.runner.abort();
        }

        Ok(TelegramAuthResponse {
            connected: false,
            waiting_for: Some("code".to_string()),
            message: "Kod yuborildi".to_string(),
        })
    }

    pub async fn verify_code(
        &self,
        code: Option<String>,
        password: Option<String>,
    ) -> Result<TelegramAuthResponse> {
        let pending = self
            .pending
            .lock()
            .await
            .take()
            .ok_or_else(|| anyhow!("Avval login kodini so'rang"))?;

        match pending.step {
            PendingStep::Code(token) => {
                let code = code
                    .as_deref()
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
                    .ok_or_else(|| anyhow!("Telegram kodi kerak"))?;

                match pending.client.sign_in(&token, code).await {
                    Ok(_) => {
                        self.set_active(pending.client, pending.runner).await;
                        Ok(TelegramAuthResponse {
                            connected: true,
                            waiting_for: None,
                            message: "Userbot ulandi".to_string(),
                        })
                    }
                    Err(SignInError::PasswordRequired(token)) => {
                        let hint = token.hint().map(|x| format!(" ({x})")).unwrap_or_default();
                        self.restore_pending(PendingLogin {
                            step: PendingStep::Password(token),
                            ..pending
                        })
                        .await;
                        Ok(TelegramAuthResponse {
                            connected: false,
                            waiting_for: Some("password".to_string()),
                            message: format!("2FA parol kerak{hint}"),
                        })
                    }
                    Err(err) => Err(anyhow!(err).context("Telegram kodini tasdiqlab bo'lmadi")),
                }
            }
            PendingStep::Password(token) => {
                let password = password
                    .as_deref()
                    .map(str::trim)
                    .filter(|x| !x.is_empty())
                    .ok_or_else(|| anyhow!("2FA parol kerak"))?;

                match pending.client.check_password(token, password).await {
                    Ok(_) => {
                        self.set_active(pending.client, pending.runner).await;
                        Ok(TelegramAuthResponse {
                            connected: true,
                            waiting_for: None,
                            message: "Userbot ulandi".to_string(),
                        })
                    }
                    Err(SignInError::InvalidPassword(token)) => {
                        self.restore_pending(PendingLogin {
                            step: PendingStep::Password(token),
                            ..pending
                        })
                        .await;
                        Err(anyhow!("2FA parol noto'g'ri"))
                    }
                    Err(err) => Err(anyhow!(err).context("2FA parolni tasdiqlab bo'lmadi")),
                }
            }
        }
    }

    pub async fn ensure_client(&self, settings: &TelegramSettings) -> Result<Client> {
        {
            let active = self.active.lock().await;
            if let Some(active) = active.as_ref() {
                if active.client.is_authorized().await.unwrap_or(false) {
                    return Ok(active.client.clone());
                }
            }
        }

        let api_id = settings
            .api_id
            .ok_or_else(|| anyhow!("Telegram API ID kiritilmagan"))?;
        let (client, runner) = self.connect(api_id).await?;
        if !client.is_authorized().await? {
            runner.abort();
            bail!("Userbot ulanmagan. Admin paneldan Telegram login qiling");
        }

        self.set_active(client.clone(), runner).await;
        Ok(client)
    }

    pub async fn get_sponsored_messages(
        &self,
        client: &Client,
        channel: &str,
        keywords: &[String],
    ) -> Result<Vec<AdResult>> {
        let username = normalize_channel(channel)
            .ok_or_else(|| anyhow!("Kanal username noto'g'ri: {channel}"))?;
        let peer = client
            .resolve_username(&username)
            .await?
            .ok_or_else(|| anyhow!("Kanal topilmadi: {channel}"))?;
        let peer_ref = peer
            .to_ref()
            .await
            .map_err(|err| anyhow!("Kanal ref olinmadi: {err}"))?
            .ok_or_else(|| anyhow!("Kanal access_hash topilmadi: {channel}"))?;

        let response = client
            .invoke(&tl::functions::messages::GetSponsoredMessages {
                peer: (&peer_ref).into(),
                msg_id: None,
            })
            .await
            .with_context(|| format!("Telegram ads olinmadi: {channel}"))?;

        let channel_title = peer.name().map(ToOwned::to_owned);
        let messages = match response {
            tl::enums::messages::SponsoredMessages::Messages(messages) => messages.messages,
            tl::enums::messages::SponsoredMessages::Empty => Vec::new(),
        };

        let mut out = Vec::new();
        for item in messages {
            let ad = match item {
                tl::enums::SponsoredMessage::Message(ad) => ad,
            };

            let matched_keywords = matched_keywords(&ad, keywords);
            if !keywords.is_empty() && matched_keywords.is_empty() {
                continue;
            }

            let random_id_hex = to_hex(&ad.random_id);
            let target_channel = normalize_channel_ref(&ad.url);
            let fingerprint = format!(
                "{}:{}:{}",
                username.to_lowercase(),
                random_id_hex,
                matched_keywords.join("|")
            );
            out.push(AdResult {
                id: uuid::Uuid::new_v4().to_string(),
                fingerprint,
                channel: username.clone(),
                channel_title: channel_title.clone(),
                target_channel,
                matched_keywords,
                title: ad.title,
                message: ad.message,
                url: ad.url,
                button_text: ad.button_text,
                sponsor_info: ad.sponsor_info,
                additional_info: ad.additional_info,
                recommended: ad.recommended,
                random_id_hex,
                found_at: chrono::Utc::now(),
            });
        }

        Ok(out)
    }

    async fn connect(&self, api_id: i32) -> Result<(Client, JoinHandle<()>)> {
        if let Some(parent) = self.session_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        let session_path = self
            .session_path
            .to_str()
            .ok_or_else(|| anyhow!("Session path UTF-8 emas"))?;
        let session = Arc::new(SqliteSession::open(session_path).await?);
        let SenderPool { runner, handle, .. } = SenderPool::new(Arc::clone(&session), api_id);
        let client = Client::new(handle);
        let runner = tokio::spawn(runner.run());
        Ok((client, runner))
    }

    async fn set_active(&self, client: Client, runner: JoinHandle<()>) {
        if let Some(old) = self
            .active
            .lock()
            .await
            .replace(ActiveClient { client, runner })
        {
            old.runner.abort();
        }
    }

    async fn restore_pending(&self, pending: PendingLogin) {
        if let Some(old) = self.pending.lock().await.replace(pending) {
            old.runner.abort();
        }
    }
}

fn normalize_channel(raw: &str) -> Option<String> {
    let value = normalize_channel_ref(raw)?;
    if value.starts_with('+') {
        None
    } else {
        Some(value)
    }
}

pub fn normalize_channel_ref(raw: &str) -> Option<String> {
    let mut value = raw.trim().trim_start_matches('@').trim().to_string();
    for prefix in [
        "https://t.me/",
        "http://t.me/",
        "t.me/",
        "https://telegram.me/",
        "telegram.me/",
    ] {
        if let Some(rest) = value.strip_prefix(prefix) {
            value = rest.to_string();
        }
    }
    value = value
        .split(['?', '/', '#'])
        .next()
        .unwrap_or_default()
        .trim_start_matches('@')
        .to_string();

    if value.is_empty() {
        None
    } else {
        Some(value.to_lowercase())
    }
}

fn matched_keywords(ad: &tl::types::SponsoredMessage, keywords: &[String]) -> Vec<String> {
    let haystack = format!(
        "{}\n{}\n{}\n{}\n{}",
        ad.title,
        ad.message,
        ad.url,
        ad.sponsor_info.as_deref().unwrap_or_default(),
        ad.additional_info.as_deref().unwrap_or_default()
    )
    .to_lowercase();

    keywords
        .iter()
        .filter_map(|keyword| {
            let clean = keyword.trim();
            if clean.is_empty() {
                None
            } else if haystack.contains(&clean.to_lowercase()) {
                Some(clean.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
