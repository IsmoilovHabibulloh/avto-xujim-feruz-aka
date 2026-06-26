use anyhow::{Context, Result, anyhow, bail};
use chrono::{DateTime, Utc};
use grammers_client::client::PasswordToken;
use grammers_client::{Client, SignInError};
use grammers_mtsender::{SenderPool, SenderPoolFatHandle};
use grammers_session::Session;
use grammers_session::storages::SqliteSession;
use grammers_tl_types::{self as tl, Deserializable, Serializable};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::models::AdResult;

/// Bir nechta Telegram akkauntini (userbot) boshqaradigan xizmat.
/// Har akkaunt o'z sessiya faylida: `<session_dir>/userbot-<id>.session`.
pub struct TelegramService {
    session_dir: PathBuf,
    clients: Mutex<HashMap<String, ActiveClient>>,
    pending: Mutex<HashMap<String, PendingQr>>,
}

struct ActiveClient {
    client: Client,
    runner: JoinHandle<()>,
}

struct PendingQr {
    client: Client,
    handle: SenderPoolFatHandle,
    session: Arc<SqliteSession>,
    runner: JoinHandle<()>,
    api_id: i32,
    api_hash: String,
    awaiting_password: bool,
}

/// QR login holatining natijasi.
pub enum QrOutcome {
    /// Hali skanерlanmagan — QR ko'rsatilib turiladi.
    Waiting {
        qr_url: String,
        expires_at: DateTime<Utc>,
    },
    /// Skanерlandi, lekin akkauntda 2FA bor — parol kerak.
    NeedPassword,
    /// Ulandi.
    Connected { username: Option<String> },
}

impl TelegramService {
    pub fn new(session_path: impl AsRef<Path>) -> Self {
        let path = session_path.as_ref();
        let session_dir = path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("data"));
        Self {
            session_dir,
            clients: Mutex::new(HashMap::new()),
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub fn account_session_path(&self, account_id: &str) -> PathBuf {
        self.session_dir.join(format!("userbot-{account_id}.session"))
    }

    /// Akkaunt uchun ulangan (avtorizatsiyalangan) klientni qaytaradi. Kesh bo'lsa
    /// undan, bo'lmasa sessiyadan ulanadi.
    pub async fn ensure_account_client(&self, account_id: &str, api_id: i32) -> Result<Client> {
        {
            let clients = self.clients.lock().await;
            if let Some(active) = clients.get(account_id) {
                if active.client.is_authorized().await.unwrap_or(false) {
                    return Ok(active.client.clone());
                }
            }
        }

        let path = self.account_session_path(account_id);
        let (client, _handle, _session, runner) = self.connect(api_id, &path).await?;
        if !client.is_authorized().await.unwrap_or(false) {
            runner.abort();
            bail!("Akkaunt ulanmagan (qayta QR kerak): {account_id}");
        }

        let mut clients = self.clients.lock().await;
        if let Some(old) = clients.insert(
            account_id.to_string(),
            ActiveClient {
                client: client.clone(),
                runner,
            },
        ) {
            old.runner.abort();
        }
        Ok(client)
    }

    /// Akkaunt keshda ulangan-ulanmaganini tekshiradi (tarmoqqa yangi ulanmaydi).
    pub async fn is_account_connected(&self, account_id: &str) -> bool {
        let clients = self.clients.lock().await;
        if let Some(active) = clients.get(account_id) {
            active.client.is_authorized().await.unwrap_or(false)
        } else {
            false
        }
    }

    /// Yangi akkaunt uchun QR login boshlaydi: token (QR url) va amal qilish vaqtini qaytaradi.
    pub async fn start_qr(
        &self,
        account_id: &str,
        api_id: i32,
        api_hash: &str,
    ) -> Result<(String, DateTime<Utc>)> {
        let path = self.account_session_path(account_id);
        let (client, handle, session, runner) = self.connect(api_id, &path).await?;

        let exported = client
            .invoke(&tl::functions::auth::ExportLoginToken {
                api_id,
                api_hash: api_hash.to_string(),
                except_ids: vec![],
            })
            .await;

        match exported {
            Ok(tl::enums::auth::LoginToken::Token(token)) => {
                let qr_url = token_to_url(&token.token);
                let expires_at = ts_to_dt(token.expires);
                self.pending.lock().await.insert(
                    account_id.to_string(),
                    PendingQr {
                        client,
                        handle,
                        session,
                        runner,
                        api_id,
                        api_hash: api_hash.to_string(),
                        awaiting_password: false,
                    },
                );
                Ok((qr_url, expires_at))
            }
            Ok(_) => {
                runner.abort();
                bail!("QR boshlashda kutilmagan javob");
            }
            Err(err) => {
                runner.abort();
                Err(anyhow!(err).context("QR token olishda xato"))
            }
        }
    }

    /// QR holatini tekshiradi: foydalanuvchi skanерladimi.
    pub async fn poll_qr(&self, account_id: &str) -> Result<QrOutcome> {
        let pending = self
            .pending
            .lock()
            .await
            .remove(account_id)
            .ok_or_else(|| anyhow!("QR sessiya topilmadi, qaytadan boshlang"))?;

        if pending.awaiting_password {
            self.pending
                .lock()
                .await
                .insert(account_id.to_string(), pending);
            return Ok(QrOutcome::NeedPassword);
        }

        let exported = pending
            .client
            .invoke(&tl::functions::auth::ExportLoginToken {
                api_id: pending.api_id,
                api_hash: pending.api_hash.clone(),
                except_ids: vec![],
            })
            .await;

        match exported {
            Ok(tl::enums::auth::LoginToken::Token(token)) => {
                let qr_url = token_to_url(&token.token);
                let expires_at = ts_to_dt(token.expires);
                self.pending
                    .lock()
                    .await
                    .insert(account_id.to_string(), pending);
                Ok(QrOutcome::Waiting { qr_url, expires_at })
            }
            Ok(tl::enums::auth::LoginToken::Success(_)) => {
                let username = self.finalize(account_id, pending).await;
                Ok(QrOutcome::Connected { username })
            }
            Ok(tl::enums::auth::LoginToken::MigrateTo(migrate)) => {
                // Akkaunt boshqa DC'da — o'sha DC'ga importLoginToken yuboramiz.
                let imported = self
                    .import_on_dc(&pending, migrate.dc_id, migrate.token)
                    .await?;
                match imported {
                    tl::enums::auth::LoginToken::Success(_) => {
                        let username = self.finalize(account_id, pending).await;
                        Ok(QrOutcome::Connected { username })
                    }
                    tl::enums::auth::LoginToken::Token(token) => {
                        let qr_url = token_to_url(&token.token);
                        let expires_at = ts_to_dt(token.expires);
                        self.pending
                            .lock()
                            .await
                            .insert(account_id.to_string(), pending);
                        Ok(QrOutcome::Waiting { qr_url, expires_at })
                    }
                    tl::enums::auth::LoginToken::MigrateTo(_) => {
                        self.pending
                            .lock()
                            .await
                            .insert(account_id.to_string(), pending);
                        bail!("DC migratsiya takrorlandi");
                    }
                }
            }
            Err(err) if rpc_is(&err, "SESSION_PASSWORD_NEEDED") => {
                let mut pending = pending;
                pending.awaiting_password = true;
                self.pending
                    .lock()
                    .await
                    .insert(account_id.to_string(), pending);
                Ok(QrOutcome::NeedPassword)
            }
            Err(err) => {
                pending.runner.abort();
                Err(anyhow!(err).context("QR holatini tekshirishda xato"))
            }
        }
    }

    /// 2FA paroli bilan QR login'ni yakunlaydi.
    pub async fn submit_qr_password(&self, account_id: &str, password: &str) -> Result<QrOutcome> {
        let pending = self
            .pending
            .lock()
            .await
            .remove(account_id)
            .ok_or_else(|| anyhow!("QR sessiya topilmadi, qaytadan boshlang"))?;

        let password_info = pending
            .client
            .invoke(&tl::functions::account::GetPassword {})
            .await
            .map_err(|err| anyhow!(err).context("2FA parol ma'lumotini olib bo'lmadi"))?;
        let tl::enums::account::Password::Password(password_info) = password_info;
        let token = PasswordToken::new(password_info);

        match pending.client.check_password(token, password).await {
            Ok(_) => {
                let username = self.finalize(account_id, pending).await;
                Ok(QrOutcome::Connected { username })
            }
            Err(SignInError::InvalidPassword(_)) => {
                let mut pending = pending;
                pending.awaiting_password = true;
                self.pending
                    .lock()
                    .await
                    .insert(account_id.to_string(), pending);
                Err(anyhow!("2FA parol noto'g'ri"))
            }
            Err(err) => {
                pending.runner.abort();
                Err(anyhow!(err).context("2FA parolni tasdiqlab bo'lmadi"))
            }
        }
    }

    pub async fn cancel_qr(&self, account_id: &str) {
        if let Some(pending) = self.pending.lock().await.remove(account_id) {
            pending.runner.abort();
        }
    }

    pub async fn disconnect_account(&self, account_id: &str) {
        if let Some(active) = self.clients.lock().await.remove(account_id) {
            active.runner.abort();
        }
        self.cancel_qr(account_id).await;
    }

    /// Akkaunt sessiyasini (fayllarini) o'chiradi.
    pub async fn remove_account_session(&self, account_id: &str) -> Result<()> {
        self.disconnect_account(account_id).await;
        let base = self.account_session_path(account_id);
        for suffix in ["", "-wal", "-shm"] {
            let p = PathBuf::from(format!("{}{}", base.display(), suffix));
            let _ = tokio::fs::remove_file(&p).await;
        }
        Ok(())
    }

    async fn import_on_dc(
        &self,
        pending: &PendingQr,
        dc_id: i32,
        token: Vec<u8>,
    ) -> Result<tl::enums::auth::LoginToken> {
        let body = tl::functions::auth::ImportLoginToken { token }.to_bytes();
        let resp = pending
            .handle
            .invoke_in_dc(dc_id, body)
            .await
            .map_err(|err| anyhow!(err).context("importLoginToken (DC) xato"))?;
        let result = tl::enums::auth::LoginToken::from_bytes(&resp)
            .map_err(|err| anyhow!("importLoginToken javobini o'qib bo'lmadi: {err}"))?;
        // Kelajakdagi ulanishlar to'g'ri DC'ga borishi uchun home DC'ni yangilaymiz.
        pending
            .session
            .set_home_dc_id(dc_id)
            .await
            .map_err(|err| anyhow!("home DC saqlanmadi: {err}"))?;
        Ok(result)
    }

    /// Pending QR klientni faol klientlar ro'yxatiga ko'chiradi va username'ni qaytaradi.
    async fn finalize(&self, account_id: &str, pending: PendingQr) -> Option<String> {
        let username = match pending.client.get_me().await {
            Ok(me) => me.username().map(|s| s.to_string()),
            Err(_) => None,
        };
        let mut clients = self.clients.lock().await;
        if let Some(old) = clients.insert(
            account_id.to_string(),
            ActiveClient {
                client: pending.client,
                runner: pending.runner,
            },
        ) {
            old.runner.abort();
        }
        username
    }

    async fn connect(
        &self,
        api_id: i32,
        session_path: &Path,
    ) -> Result<(Client, SenderPoolFatHandle, Arc<SqliteSession>, JoinHandle<()>)> {
        if let Some(parent) = session_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let sp = session_path
            .to_str()
            .ok_or_else(|| anyhow!("Session path UTF-8 emas"))?;
        let session = Arc::new(SqliteSession::open(sp).await?);
        let SenderPool { runner, handle, .. } = SenderPool::new(Arc::clone(&session), api_id);
        let client = Client::new(handle.clone());
        let runner = tokio::spawn(runner.run());
        Ok((client, handle, session, runner))
    }

    /// Berilgan `query` (key) bo'yicha GLOBAL sponsored qidiruv (`contacts.getSponsoredPeers`).
    pub async fn get_sponsored_peers(&self, client: &Client, query: &str) -> Result<Vec<AdResult>> {
        let query_trimmed = query.trim();
        if query_trimmed.is_empty() {
            return Ok(Vec::new());
        }

        let response = client
            .invoke(&tl::functions::contacts::GetSponsoredPeers {
                q: query_trimmed.to_string(),
            })
            .await
            .with_context(|| format!("Telegram sponsored qidiruv xatosi: {query_trimmed}"))?;

        let data = match response {
            tl::enums::contacts::SponsoredPeers::Peers(data) => data,
            tl::enums::contacts::SponsoredPeers::Empty => return Ok(Vec::new()),
        };

        let query_lc = query_trimmed.to_lowercase();
        let mut out = Vec::new();

        for peer in data.peers {
            let tl::enums::SponsoredPeer::Peer(peer) = peer;
            let Some((username, title)) = resolve_peer(&peer.peer, &data.chats, &data.users) else {
                continue;
            };

            let username_lc = username.to_lowercase();
            let url = format!("https://t.me/{username}");
            let random_id_hex = to_hex(&peer.random_id);
            let fingerprint = format!("{query_lc}:{username_lc}");

            out.push(AdResult {
                id: uuid::Uuid::new_v4().to_string(),
                fingerprint,
                channel: username_lc.clone(),
                channel_title: title.clone(),
                target_channel: Some(username_lc),
                matched_keywords: vec![query_trimmed.to_string()],
                title: title.unwrap_or_default(),
                message: peer.additional_info.clone().unwrap_or_default(),
                url,
                button_text: String::new(),
                sponsor_info: peer.sponsor_info,
                additional_info: peer.additional_info,
                recommended: false,
                random_id_hex,
                found_at: chrono::Utc::now(),
            });
        }

        Ok(out)
    }
}

/// RPC xatosining nomi berilganga mosligini tekshiradi (xato zanjiri bo'ylab).
fn rpc_is(err: &grammers_mtsender::InvocationError, name: &str) -> bool {
    matches!(err, grammers_mtsender::InvocationError::Rpc(rpc) if rpc.name == name)
}

fn token_to_url(token: &[u8]) -> String {
    format!("tg://login?token={}", base64url(token))
}

fn ts_to_dt(secs: i32) -> DateTime<Utc> {
    DateTime::from_timestamp(secs as i64, 0).unwrap_or_else(Utc::now)
}

fn base64url(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = *chunk.get(1).unwrap_or(&0) as u32;
        let b2 = *chunk.get(2).unwrap_or(&0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 63) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 63) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[((n >> 6) & 63) as usize] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(n & 63) as usize] as char);
        }
    }
    out
}

fn resolve_peer(
    peer: &tl::enums::Peer,
    chats: &[tl::enums::Chat],
    users: &[tl::enums::User],
) -> Option<(String, Option<String>)> {
    match peer {
        tl::enums::Peer::Channel(p) => {
            for chat in chats {
                if let tl::enums::Chat::Channel(c) = chat {
                    if c.id == p.channel_id {
                        return primary_username(c.username.as_deref(), c.usernames.as_deref())
                            .map(|name| (name, Some(c.title.clone())));
                    }
                }
            }
            None
        }
        tl::enums::Peer::User(p) => {
            for user in users {
                if let tl::enums::User::User(u) = user {
                    if u.id == p.user_id {
                        return primary_username(u.username.as_deref(), u.usernames.as_deref())
                            .map(|name| (name, u.first_name.clone()));
                    }
                }
            }
            None
        }
        tl::enums::Peer::Chat(_) => None,
    }
}

fn primary_username(
    username: Option<&str>,
    usernames: Option<&[tl::enums::Username]>,
) -> Option<String> {
    if let Some(value) = username {
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    if let Some(list) = usernames {
        for entry in list {
            let tl::enums::Username::Username(entry) = entry;
            if entry.active && !entry.username.is_empty() {
                return Some(entry.username.clone());
            }
        }
    }
    None
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

fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(HEX[(byte >> 4) as usize] as char);
        out.push(HEX[(byte & 0x0f) as usize] as char);
    }
    out
}
