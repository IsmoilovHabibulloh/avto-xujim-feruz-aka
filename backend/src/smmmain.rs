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

#[derive(Clone, Debug)]
pub struct SmmBalanceOutcome {
    pub balance: Option<String>,
    pub currency: Option<String>,
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

    pub fn is_configured(&self) -> bool {
        !self.api_key.trim().is_empty()
    }

    pub async fn send_order(
        &self,
        service_id: u64,
        link: &str,
        quantity: u64,
    ) -> Result<SmmOrderOutcome> {
        if !self.is_configured() {
            bail!("BAZA kaliti (.env) kiritilmagan");
        }

        let service = if service_id == 0 {
            self.service_id
        } else {
            service_id
        }
        .to_string();
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
            .context("BAZA API ga ulanishda xatolik")?;

        let status = response.status();
        let raw_response = response
            .text()
            .await
            .context("BAZA javobini o'qib bo'lmadi")?;

        if !status.is_success() {
            bail!("BAZA HTTP {status}: {raw_response}");
        }

        let value = serde_json::from_str::<Value>(&raw_response)
            .with_context(|| format!("BAZA JSON javobi tushunarsiz: {raw_response}"))?;

        if let Some(error) = value.get("error").and_then(Value::as_str) {
            return Err(anyhow!("BAZA xato qaytardi: {error}"));
        }

        // Muvaffaqiyatli javobda doim "order" id bo'ladi. Bo'lmasa, orderni
        // muvaffaqiyatli deb hisoblamaymiz (aks holda rad etilgan order ham
        // "yuborildi" deb belgilanib qolardi).
        let order_id = value.get("order").map(value_to_string);
        if order_id.is_none() {
            return Err(anyhow!(
                "BAZA order id qaytarmadi, javob: {raw_response}"
            ));
        }

        Ok(SmmOrderOutcome {
            order_id,
            raw_response,
        })
    }

    pub async fn balance(&self) -> Result<SmmBalanceOutcome> {
        if !self.is_configured() {
            bail!("BAZA kaliti (.env) kiritilmagan");
        }

        let form = [("key", self.api_key.trim()), ("action", "balance")];
        let response = self
            .http
            .post(self.api_url.trim())
            .form(&form)
            .send()
            .await
            .context("BAZA balans API ga ulanishda xatolik")?;

        let status = response.status();
        let raw_response = response
            .text()
            .await
            .context("BAZA balans javobini o'qib bo'lmadi")?;

        if !status.is_success() {
            bail!("BAZA balans HTTP {status}: {raw_response}");
        }

        let value = serde_json::from_str::<Value>(&raw_response)
            .with_context(|| format!("BAZA balans JSON javobi tushunarsiz: {raw_response}"))?;

        if let Some(error) = value.get("error").and_then(Value::as_str) {
            return Err(anyhow!("BAZA balans xato qaytardi: {error}"));
        }

        Ok(SmmBalanceOutcome {
            balance: value.get("balance").map(value_to_string),
            currency: value.get("currency").map(value_to_string),
        })
    }
}

fn value_to_string(value: &Value) -> String {
    match value {
        Value::String(value) => value.clone(),
        _ => value.to_string(),
    }
}
