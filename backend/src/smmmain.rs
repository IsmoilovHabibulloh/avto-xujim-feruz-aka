use anyhow::{Context, Result, anyhow, bail};
use reqwest::Client;
use serde_json::Value;

#[derive(Clone)]
pub struct SmmMainService {
    api_key: String,
    api_url: String,
    service_id: u64,
    http: Client,
}

#[derive(Clone, Debug)]
pub struct SmmOrderOutcome {
    pub order_id: Option<String>,
    pub raw_response: String,
}

impl SmmMainService {
    pub fn new(api_key: String, api_url: String, service_id: u64) -> Self {
        Self {
            api_key,
            api_url,
            service_id,
            http: Client::new(),
        }
    }

    pub fn service_id(&self) -> u64 {
        self.service_id
    }

    pub fn is_configured(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    pub async fn send_order(&self, link: &str, quantity: u64) -> Result<SmmOrderOutcome> {
        if !self.is_configured() {
            bail!("SMMMAIN_API_KEY .env ichida kiritilmagan");
        }

        let service = self.service_id.to_string();
        let quantity = quantity.to_string();
        let form = [
            ("key", self.api_key.trim()),
            ("action", "add"),
            ("service", service.as_str()),
            ("link", link.trim()),
            ("quantity", quantity.as_str()),
        ];

        let response = self
            .http
            .post(self.api_url.trim())
            .form(&form)
            .send()
            .await
            .context("SMMMAIN API ga ulanishda xatolik")?;

        let status = response.status();
        let raw_response = response
            .text()
            .await
            .context("SMMMAIN javobini o'qib bo'lmadi")?;

        if !status.is_success() {
            bail!("SMMMAIN HTTP {status}: {raw_response}");
        }

        let value = serde_json::from_str::<Value>(&raw_response)
            .with_context(|| format!("SMMMAIN JSON javobi tushunarsiz: {raw_response}"))?;

        if let Some(error) = value.get("error").and_then(Value::as_str) {
            return Err(anyhow!("SMMMAIN xato qaytardi: {error}"));
        }

        let order_id = value.get("order").map(value_to_string);
        Ok(SmmOrderOutcome {
            order_id,
            raw_response,
        })
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}
