use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
#[derive(Clone)]
pub struct Settings {
    pub chat: i64,
    pub model: String,
    pub telegram_token: String,
    pub openai_key: String,
    pub upscale_key: Option<String>,
    pub max_tokens: u32,
    pub database: PathBuf,
    pub strategy: PathBuf,
    pub stale_seconds: i64,
}
impl Settings {
    pub fn load(path: &Path) -> Result<Self, String> {
        Self::parse(&std::fs::read_to_string(path).map_err(|_| "cannot read service settings")?)
    }
    pub fn parse(text: &str) -> Result<Self, String> {
        let names = [
            "TELEGRAM_BOT_TOKEN",
            "TELEGRAM_CHAT_ID",
            "OPENAI_API_KEY",
            "OPENAI_MODEL",
            "OPENAI_MAX_TOKENS",
            "DATABASE_PATH",
            "STRATEGY_PATH",
            "QUOTE_STALE_SECONDS",
            "PRICE_TICK",
            "UPSCALE_API_KEY",
        ];
        let mut values = BTreeMap::new();
        for line in text
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let (key, value) = line.split_once('=').ok_or("invalid settings line")?;
            let key = key.trim();
            let mut value = value.trim();
            if !names.contains(&key) {
                return Err("unknown setting name".into());
            }
            if (value.starts_with('\'') && value.ends_with('\''))
                || (value.starts_with('"') && value.ends_with('"'))
            {
                if value.len() < 2 {
                    return Err("invalid quoted setting".into());
                }
                value = &value[1..value.len() - 1];
            }
            if value.is_empty() {
                return Err(format!("missing setting: {key}"));
            }
            if values.insert(key, value).is_some() {
                return Err(format!("duplicate setting: {key}"));
            }
        }
        for name in names {
            if !matches!(name, "PRICE_TICK" | "UPSCALE_API_KEY") && !values.contains_key(name) {
                return Err(format!("missing setting: {name}"));
            }
        }
        let number = |key: &str| {
            values[key]
                .parse::<i64>()
                .map_err(|_| format!("invalid setting: {key}"))
        };
        let chat = number("TELEGRAM_CHAT_ID")?;
        let stale_seconds = number("QUOTE_STALE_SECONDS")?;
        if values.contains_key("PRICE_TICK") && number("PRICE_TICK")? <= 0 {
            return Err("invalid legacy PRICE_TICK".into());
        }
        let max_tokens =
            u32::try_from(number("OPENAI_MAX_TOKENS")?).map_err(|_| "invalid OPENAI_MAX_TOKENS")?;
        if chat == 0 || stale_seconds <= 0 || max_tokens == 0 {
            return Err("invalid service numeric settings".into());
        }
        Ok(Self {
            chat,
            model: values["OPENAI_MODEL"].into(),
            telegram_token: values["TELEGRAM_BOT_TOKEN"].into(),
            openai_key: values["OPENAI_API_KEY"].into(),
            upscale_key: values.get("UPSCALE_API_KEY").map(|key| (*key).to_owned()),
            max_tokens,
            database: values["DATABASE_PATH"].into(),
            strategy: values["STRATEGY_PATH"].into(),
            stale_seconds,
        })
    }
}
