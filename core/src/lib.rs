pub mod calendar;
pub mod contract;
pub mod document;
pub mod engine;
pub mod feed;
pub mod model;
pub mod prices;
pub mod processing;
pub mod trading;
use model::Strategy;
use serde_json::{json, Value};
use std::{collections::HashSet, fs, path::Path};
pub fn load_document(path: &Path) -> Result<Value, String> {
    let bytes = fs::read(path).map_err(|_| "read: cannot read strategy file")?;
    let v = serde_json::from_slice::<document::StrictValue>(&bytes)
        .map_err(|e| {
            format!(
                "structure: invalid JSON at line {}, column {}",
                e.line(),
                e.column()
            )
        })?
        .0;
    if v.get("schema_version").and_then(Value::as_u64) != Some(2) {
        return Err("unsupported: strategy schema_version; expected 2".into());
    }
    let c: Strategy = serde_json::from_value(v.clone())
        .map_err(|_| "structure: invalid fields, types or policies")?;
    validate(&c)?;
    Ok(v)
}
pub fn load(path: &Path) -> Result<Strategy, String> {
    serde_json::from_value(load_document(path)?).map_err(|_| "structure: invalid strategy".into())
}
pub fn valid_symbol(s: &str) -> bool {
    s.len() == 6 && s.bytes().all(|b| b.is_ascii_uppercase())
}
pub fn validate(c: &Strategy) -> Result<(), String> {
    if c.schema_version != 2 {
        return Err("unsupported: schema_version".into());
    }
    let mut symbols = HashSet::new();
    if c.instruments.is_empty()
        || c.instruments
            .iter()
            .any(|i| !valid_symbol(&i.symbol) || i.price_tick <= 0 || !symbols.insert(&i.symbol))
    {
        return Err("invalid instrument".into());
    }
    let v = serde_json::to_value(c).map_err(|_| "structure: serialization")?;
    for spec in contract::PARAMETERS {
        let value = &v["parameters"][spec.name]["value"];
        if let Some(min) = spec.minimum {
            let n = value.as_f64().ok_or("structure: numeric parameter")?;
            if !n.is_finite() || n < min || spec.maximum.is_some_and(|m| n > m) {
                return Err(format!("range: parameters.{}", spec.name));
            }
        }
        if spec.kind == "string"
            && !spec.allowed.is_empty()
            && !value.as_str().is_some_and(|s| spec.allowed.contains(&s))
        {
            return Err(format!("unsupported: parameters.{}", spec.name));
        }
    }
    for x in [
        c.parameters.rr_min.value,
        c.parameters.rr_max_trend.value,
        c.parameters.rr_max_correction.value,
    ] {
        trading::rr_ratio(x)?;
    }
    if c.parameters.rr_min.value > c.parameters.rr_max_trend.value
        || c.parameters.rr_min.value > c.parameters.rr_max_correction.value
    {
        return Err("range: RR bounds".into());
    }
    for tf in ["W", "D", "H4"] {
        let min = v["context"]["minimum_bars"][tf].as_u64().unwrap();
        let bars = v["context"]["bars_analyzed"][tf].as_u64().unwrap();
        if min == 0 || bars == 0 {
            return Err("range: context".into());
        }
        if min > bars {
            return Err("inconsistent: context".into());
        }
    }
    let statuses = &c.parameters.plan_days_statuses.value;
    let mut unique = HashSet::new();
    if statuses.is_empty()
        || statuses.iter().any(|s| {
            !["wait_confirm", "wait_entry", "setup"].contains(&s.as_str()) || !unique.insert(s)
        })
    {
        return Err("unsupported: plan_days_statuses".into());
    }
    if !matches!(c.candles.price_basis.as_str(), "bid" | "index") {
        return Err("unsupported: /candles/price_basis".into());
    }
    for (path, expected) in [
        ("/instrument_price_tick_scale", json!(1_000_000)),
        ("/candles/timezone", json!("America/New_York")),
        ("/candles/only_closed_bars", json!(true)),
        ("/candles/h4_close_hours", json!([1, 5, 9, 13, 17, 21])),
        ("/candles/daily_boundary", json!("17:00")),
        ("/candles/weekly_boundary/time", json!("17:00")),
        ("/candles/weekly_boundary/weekday", json!("Sunday")),
        ("/policies/plans/daily_requires_weekly_trend", json!(true)),
        ("/policies/modules/range_trading", json!(false)),
        ("/policies/modules/daily_point_a", json!(false)),
        ("/policies/modules/weak_correction", json!(false)),
        ("/policies/risk/max_open_risk_r", Value::Null),
        ("/policies/risk/per_trade_r", json!(1.0)),
        (
            "/policies/risk/queue_order",
            json!(["signal_time", "rr_desc", "instrument_order"]),
        ),
        ("/policies/trade_window/session_filter", json!(false)),
        (
            "/policies/targets/weekly_poi_daily_alternative/enabled",
            json!(true),
        ),
        (
            "/policies/targets/weekly_poi_daily_alternative/must_be_closer",
            json!(true),
        ),
    ] {
        if v.pointer(path) != Some(&expected) {
            return Err(format!("unsupported: {path}"));
        }
    }
    for (path, len) in [
        ("/policies/plans/weekly_rules", 3),
        ("/policies/plans/daily_rules", 9),
        ("/policies/plans/confirmation", 2),
        ("/policies/plans/entry", 2),
        ("/policies/targets/types", 2),
        ("/policies/execution/pending_cancel_on", 3),
        ("/policies/risk/counted_statuses", 2),
    ] {
        let a = v.pointer(path).and_then(Value::as_array).unwrap();
        let mut seen = HashSet::new();
        if a.len() != len || a.iter().any(|s| !seen.insert(s.to_string())) {
            return Err(format!("rule: missing or duplicate policy: {path}"));
        }
    }
    if [
        &c.messages.w_trend_new,
        &c.messages.w_trend_end,
        &c.messages.w_trend_flip,
    ]
    .iter()
    .any(|s| s.trim().is_empty())
    {
        return Err("structure: empty message".into());
    }
    Ok(())
}
