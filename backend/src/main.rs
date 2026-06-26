mod api;
mod models;
mod scanner;
mod smmmain;
mod store;
mod telegram;

use crate::api::{AppState, RuntimeInfo};
use crate::smmmain::SmmMainService;
use crate::store::Store;
use crate::telegram::TelegramService;
use anyhow::{Context, Result};
use axum::Router;
use std::collections::HashMap;
use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::cors::CorsLayer;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    init_tracing();

    let state_path = env::var("STATE_PATH").unwrap_or_else(|_| "data/state.json".to_string());
    let session_path =
        env::var("TELEGRAM_SESSION_PATH").unwrap_or_else(|_| "data/userbot.session".to_string());
    let static_dir = env::var("STATIC_DIR").unwrap_or_else(|_| "frontend/dist".to_string());
    let smmmain_api_key = env::var("SMMMAIN_API_KEY").unwrap_or_default();
    let smmmain_api_url =
        env::var("SMMMAIN_API_URL").unwrap_or_else(|_| "https://smmmain.com/api/v2".to_string());

    let store = Arc::new(Store::load(state_path).await?);
    seed_telegram_from_env(&store).await?;

    let app_state = AppState {
        store,
        telegram: Arc::new(TelegramService::new(session_path)),
        smmmain: Arc::new(SmmMainService::new(smmmain_api_key, smmmain_api_url, 875)),
        sessions: Arc::new(RwLock::new(HashMap::new())),
        runtime: Arc::new(RwLock::new(RuntimeInfo::default())),
        admin_username: env::var("ADMIN_USERNAME").unwrap_or_else(|_| "Izzatillo".to_string()),
        admin_password: env::var("ADMIN_PASSWORD").unwrap_or_else(|_| "Izzatilloaka".to_string()),
    };

    tokio::spawn(scanner::scanner_loop(app_state.clone()));

    let app = build_router(app_state, PathBuf::from(static_dir));
    let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = env::var("PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()
        .context("PORT noto'g'ri")?;
    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .context("HOST/PORT noto'g'ri")?;

    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!("VIP Ads server ishga tushdi: http://{addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn seed_telegram_from_env(store: &Store) -> Result<()> {
    let api_id = env::var("TELEGRAM_API_ID")
        .ok()
        .and_then(|value| value.trim().parse::<i32>().ok());
    let api_hash = env::var("TELEGRAM_API_HASH")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    let phone = env::var("TELEGRAM_PHONE")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    if api_id.is_none() && api_hash.is_none() && phone.is_none() {
        return Ok(());
    }

    let mut settings = store.telegram_settings().await;
    if let Some(api_id) = api_id {
        settings.api_id = Some(api_id);
    }
    if let Some(api_hash) = api_hash {
        settings.api_hash = Some(api_hash);
    }
    if let Some(phone) = phone {
        settings.phone = Some(phone);
    }

    store.update_telegram(settings).await?;
    Ok(())
}

fn build_router(state: AppState, static_dir: PathBuf) -> Router {
    let index = static_dir.join("index.html");
    let frontend = ServeDir::new(static_dir).not_found_service(ServeFile::new(index));

    Router::new()
        .nest("/api", api::router(state))
        .fallback_service(frontend)
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
}

fn init_tracing() {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("vipads_server=info,tower_http=info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}
