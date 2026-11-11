use reqwest::blocking::{Client, Response};
use serde_json::Value;
use std::{io::Read, time::Duration};
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Failure {
    Access,
    Retry,
    Invalid,
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
fn status(code: u16) -> Failure {
    match code {
        401 | 403 => Failure::Access,
        408 | 429 | 500..=599 => Failure::Retry,
        _ => Failure::Invalid,
    }
}
pub struct Api {
    client: Client,
}
impl Api {
    pub fn market(&self, symbol: &str) -> Result<Value, Failure> {
        Self::json(
            self.client
                .get(format!("https://biquote.io/api/{symbol}"))
                .send()
                .map_err(|_| Failure::Retry)?,
        )
    }

    pub fn new() -> Result<Self, Failure> {
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(45))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| Failure::Invalid)?;
        Ok(Self { client })
    }
    fn bytes(response: Response, limit: u64) -> Result<Vec<u8>, Failure> {
        if !response.status().is_success() {
            return Err(status(response.status().as_u16()));
        }
        if response.content_length().is_some_and(|n| n > limit) {
            return Err(Failure::Invalid);
        }
        let mut bytes = Vec::new();
        response
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Failure::Retry)?;
        if bytes.len() as u64 > limit {
            return Err(Failure::Invalid);
        }
        Ok(bytes)
    }
    fn json(response: Response) -> Result<Value, Failure> {
        serde_json::from_slice(&Self::bytes(response, 2 * 1024 * 1024)?)
            .map_err(|_| Failure::Invalid)
    }
    pub fn telegram(
        &self,
        base: &str,
        token: &str,
        method: &str,
        body: &Value,
    ) -> Result<Value, Failure> {
        if !matches!(
            method,
            "getUpdates" | "sendMessage" | "getFile" | "getMe" | "getChat"
        ) || token.is_empty()
        {
            return Err(Failure::Invalid);
        }
        let response = self
            .client
            .post(format!("{base}/bot{token}/{method}"))
            .json(body)
            .send()
            .map_err(|_| Failure::Retry)?;
        let v = Self::json(response)?;
        if v["ok"] != true {
            return Err(status(
                v["error_code"]
                    .as_u64()
                    .and_then(|v| u16::try_from(v).ok())
                    .unwrap_or(400),
            ));
        }
        v.get("result").cloned().ok_or(Failure::Invalid)
    }
    pub fn responses(&self, base: &str, key: &str, body: &Value) -> Result<Value, Failure> {
        let response = self
            .client
            .post(format!("{base}/v1/responses"))
            .bearer_auth(key)
            .json(body)
            .send()
            .map_err(|_| Failure::Retry)?;
        Self::json(response)
    }
    pub fn file(&self, base: &str, token: &str, path: &str) -> Result<Vec<u8>, Failure> {
        if path.is_empty()
            || path.starts_with('/')
            || path.split('/').any(|p| p == ".." || p.is_empty())
            || !path
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "_-/ .".contains(c))
            || path.contains(' ')
        {
            return Err(Failure::Invalid);
        }
        let response = self
            .client
            .get(format!("{base}/file/bot{token}/{path}"))
            .send()
            .map_err(|_| Failure::Retry)?;
        Self::bytes(response, 20 * 1024 * 1024)
    }
}
