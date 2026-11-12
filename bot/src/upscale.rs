//! Read-only Upscale index-price snapshots, separate from Bid/Ask ticks.
use serde::Serialize;
use serde_json::Value;

pub const BASE_URL: &str = "https://api.upscale.trade";
#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    Invalid(&'static str),
    Http {
        status: u16,
        retry_after_seconds: Option<u64>,
    },
    Transport,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Upscale: {self:?}")
    }
}
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct IndexQuote {
    pub symbol: String,
    pub market_id: String,
    pub ticker: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub index_price_fp9: i64,
    pub millis: i64,
    pub source: &'static str,
    pub closed: bool,
    pub paused: bool,
    pub close_only: bool,
}

fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, c)| {
            if [8, 13, 18, 23].contains(&i) {
                c == b'-'
            } else {
                c.is_ascii_hexdigit()
            }
        })
}
fn identity(symbol: &str) -> Result<(&'static str, &'static str), Error> {
    match symbol {
        "EURUSD" => Ok(("EUR/USD", "EUR")),
        "GBPUSD" => Ok(("GBP/USD", "GBP")),
        "USDJPY" => Ok(("USDJPY/USD", "USDJPY")),
        _ => Err(Error::Invalid("unsupported symbol")),
    }
}
pub fn parse_quotes(
    value: &Value,
    symbols: &[String],
    now_ms: i64,
    stale_seconds: i64,
) -> Result<Vec<IndexQuote>, Error> {
    let stale_ms = stale_seconds
        .checked_mul(1000)
        .filter(|n| *n > 0)
        .ok_or(Error::Invalid("invalid freshness limit"))?;
    if now_ms < 0 || symbols.is_empty() {
        return Err(Error::Invalid("invalid quote request"));
    }
    let markets = value
        .as_array()
        .ok_or(Error::Invalid("expected markets array"))?;
    let mut seen = std::collections::HashSet::new();
    symbols
        .iter()
        .map(|symbol| {
            if !seen.insert(symbol) {
                return Err(Error::Invalid("duplicate symbol"));
            }
            let (ticker, base) = identity(symbol)?;
            let mut matches = markets.iter().filter(|m| m["config"]["ticker"] == ticker);
            let market = matches.next().ok_or(Error::Invalid("missing market"))?;
            if matches.next().is_some() {
                return Err(Error::Invalid("ambiguous market"));
            }
            if market["config"]["baseAsset"] != base || market["config"]["quoteAsset"] != "USD" {
                return Err(Error::Invalid("unexpected market identity"));
            }
            let id = market["id"]
                .as_str()
                .filter(|id| uuid(id))
                .ok_or(Error::Invalid("invalid market ID"))?;
            let raw = market["state"]["indexPrice"]
                .as_str()
                .ok_or(Error::Invalid("expected fp9 string"))?;
            if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) || raw.starts_with('0') {
                return Err(Error::Invalid("invalid fp9 price"));
            }
            let price = raw
                .parse::<i64>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or(Error::Invalid("invalid fp9 price"))?;
            let millis = market["state"]["blockTimestamp"]
                .as_i64()
                .filter(|n| {
                    *n > 0 && *n <= now_ms && chrono::DateTime::from_timestamp_millis(*n).is_some()
                })
                .ok_or(Error::Invalid("invalid or future snapshot time"))?;
            let flag = |name: &str| {
                market["settings"][name]
                    .as_bool()
                    .ok_or(Error::Invalid("missing market status"))
            };
            let closed = flag("isClosed")?;
            let paused = flag("isPaused")?;
            let close_only = flag("isCloseOnly")?;
            if !closed && !paused && now_ms - millis > stale_ms {
                return Err(Error::Invalid("stale snapshot"));
            }
            Ok(IndexQuote {
                symbol: symbol.clone(),
                market_id: id.into(),
                ticker: ticker.into(),
                base_asset: base.into(),
                quote_asset: "USD".into(),
                index_price_fp9: price,
                millis,
                source: "upscale",
                closed,
                paused,
                close_only,
            })
        })
        .collect()
}

pub struct Client {
    http: reqwest::blocking::Client,
    base: String,
    key: String,
}
impl Client {
    pub fn new(base: &str, key: &str) -> Result<Self, Error> {
        let url = url::Url::parse(base).map_err(|_| Error::Invalid("invalid API base"))?;
        let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
        if (url.scheme() != "https" && !(url.scheme() == "http" && loopback))
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err(Error::Invalid("invalid API base"));
        }
        if key.trim().is_empty() || key.bytes().any(|b| b.is_ascii_control()) {
            return Err(Error::Invalid("missing or invalid API key"));
        }
        let http = reqwest::blocking::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| Error::Transport)?;
        Ok(Self {
            http,
            base: base.trim_end_matches('/').into(),
            key: key.into(),
        })
    }
    fn get(&self, path: &str, account: Option<&str>) -> Result<Value, Error> {
        use std::io::Read;
        let mut request = self
            .http
            .get(format!("{}{path}", self.base))
            .bearer_auth(&self.key)
            .header("Accept", "application/json");
        if let Some(id) = account {
            request = request.query(&[("accountId", id)]);
        }
        let response = request.send().map_err(|_| Error::Transport)?;
        if !response.status().is_success() {
            return Err(Error::Http {
                status: response.status().as_u16(),
                retry_after_seconds: response
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse().ok()),
            });
        }
        const LIMIT: u64 = 2 * 1024 * 1024;
        if response.content_length().is_some_and(|n| n > LIMIT) {
            return Err(Error::Invalid("response too large"));
        }
        let mut bytes = Vec::new();
        response
            .take(LIMIT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::Transport)?;
        if bytes.len() as u64 > LIMIT {
            return Err(Error::Invalid("response too large"));
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::Invalid("invalid JSON response"))
    }
    pub fn demo_accounts(&self) -> Result<Vec<String>, Error> {
        let value = self.get("/accounts/with-risk-status", None)?;
        let accounts = value
            .as_array()
            .ok_or(Error::Invalid("expected accounts array"))?;
        let mut ids = Vec::new();
        for a in accounts
            .iter()
            .filter(|a| a["type"] == "demo" && a["status"] == "active" && a["apiTrading"] == true)
        {
            let id = a["accountId"]
                .as_str()
                .filter(|id| uuid(id))
                .ok_or(Error::Invalid("invalid account ID"))?
                .to_owned();
            if ids.contains(&id) {
                return Err(Error::Invalid("duplicate account ID"));
            }
            ids.push(id);
        }
        if ids.is_empty() {
            return Err(Error::Invalid("no active API-enabled demo account"));
        }
        Ok(ids)
    }
    pub fn markets(&self, account: &str) -> Result<Value, Error> {
        if !uuid(account) {
            return Err(Error::Invalid("invalid account ID"));
        }
        self.get("/v2/markets", Some(account))
    }
    pub fn quotes(
        &self,
        account: &str,
        symbols: &[String],
        now_ms: impl FnOnce() -> i64,
        stale_seconds: i64,
    ) -> Result<Vec<IndexQuote>, Error> {
        if !uuid(account) {
            return Err(Error::Invalid("invalid account ID"));
        }
        for symbol in symbols {
            identity(symbol)?;
        }
        let value = self.markets(account)?;
        parse_quotes(&value, symbols, now_ms(), stale_seconds)
    }
}

pub fn probe(path: &std::path::Path) -> Result<String, String> {
    let settings = crate::settings::Settings::load(path)?;
    let key = settings
        .upscale_key
        .as_deref()
        .ok_or("missing UPSCALE_API_KEY")?;
    let strategy = crate::load(&settings.strategy)?;
    let symbols: Vec<_> = strategy
        .instruments
        .iter()
        .map(|i| i.symbol.clone())
        .collect();
    let client = Client::new(BASE_URL, key).map_err(|e| e.to_string())?;
    let accounts = client.demo_accounts().map_err(|e| e.to_string())?;
    let mut reports = Vec::new();
    for (index, account) in accounts.iter().enumerate() {
        let clock = || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|d| i64::try_from(d.as_millis()).ok())
                .unwrap_or(-1)
        };
        let quotes = client
            .quotes(account, &symbols, clock, settings.stale_seconds)
            .map_err(|e| e.to_string())?;
        reports.push(serde_json::json!({"demo_number":index+1,"price_basis":"index","price_scale":1_000_000_000,"quotes":quotes}));
    }
    serde_json::to_string_pretty(&reports).map_err(|_| "cannot serialize Upscale snapshots".into())
}

impl IndexQuote {
    pub fn to_tick(&self) -> Result<crate::feed::Tick, Error> {
        if self.closed || self.paused || self.index_price_fp9 <= 0 {
            return Err(Error::Invalid("market is unavailable"));
        }
        let price = self.index_price_fp9 / 1000 + i64::from(self.index_price_fp9 % 1000 >= 500);
        if price <= 0 {
            return Err(Error::Invalid("price below engine precision"));
        }
        Ok(crate::feed::Tick {
            symbol: self.symbol.clone(),
            millis: self.millis,
            bid: price,
            ask: price,
            source: "upscale:index".into(),
        })
    }
}
