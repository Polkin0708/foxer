//! v2 field capabilities: shared by validation, diagnostics and Telegram patches.
use serde::Serialize;
use serde_json::{json, Value};
#[derive(Debug, Serialize)]
pub struct ParameterSpec {
    pub name: &'static str,
    pub kind: &'static str,
    pub minimum: Option<f64>,
    pub maximum: Option<f64>,
    pub allowed: &'static [&'static str],
    pub telegram_editable: bool,
}
macro_rules! num {
    ($n:literal,$k:literal,$min:expr,$max:expr) => {
        ParameterSpec {
            name: $n,
            kind: $k,
            minimum: Some($min),
            maximum: Some($max),
            allowed: &[],
            telegram_editable: true,
        }
    };
}
macro_rules! choice {($n:literal,[$($v:literal),+])=>{ParameterSpec{name:$n,kind:"string",minimum:None,maximum:None,allowed:&[$($v),+],telegram_editable:true}};}
pub const PARAMETERS: &[ParameterSpec] = &[
    num!("atr_period", "integer", 1.0, usize::MAX as f64),
    choice!("atr_reference_zone", ["formation_bar", "last_closed_bar"]),
    choice!(
        "correction_entry_zones_from",
        ["plan_day_open", "plan_start_minus_24h"]
    ),
    ParameterSpec {
        name: "ifvg_entry",
        kind: "boolean",
        minimum: None,
        maximum: None,
        allowed: &[],
        telegram_editable: true,
    },
    choice!("ifvg_limit_range", ["inside_zone", "close_to_far_edge"]),
    choice!(
        "ifvg_max_zone_age",
        ["plan_day_open", "plan_start_minus_24h"]
    ),
    num!("inv_margin_atr", "number", 0.0, f64::MAX),
    num!("min_fvg_atr", "number", 0.0, f64::MAX),
    num!("trend_fvg_atr", "number", 0.0, f64::MAX),
    num!("stop_buffer_atr", "number", 0.0, f64::MAX),
    num!("plan_days", "integer", 1.0, (i64::MAX / 86400) as f64),
    ParameterSpec {
        name: "plan_days_statuses",
        kind: "array",
        minimum: None,
        maximum: None,
        allowed: &["wait_confirm", "wait_entry", "setup"],
        telegram_editable: false,
    },
    num!("rr_min", "number", 0.000000001, 1000000.0),
    num!("rr_max_trend", "number", 0.000000001, 1000000.0),
    num!("rr_max_correction", "number", 0.000000001, 1000000.0),
    num!("stop_fallback_h4_bars", "integer", 1.0, usize::MAX as f64),
    num!(
        "confirmation_inversion_h4_bars",
        "integer",
        1.0,
        usize::MAX as f64
    ),
    num!(
        "weekly_fractal_sweep_lookback_bars",
        "integer",
        1.0,
        usize::MAX as f64
    ),
    num!(
        "target_fractal_lookback_d_bars",
        "integer",
        1.0,
        usize::MAX as f64
    ),
    num!(
        "target_fractal_lookback_w_bars",
        "integer",
        1.0,
        usize::MAX as f64
    ),
];
fn escape(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}
pub fn comment_paths(v: &Value) -> Vec<String> {
    fn visit(v: &Value, p: &str, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                out.push(format!("{p}/comment"));
                for (k, v) in m {
                    if k != "comment" {
                        visit(v, &format!("{p}/{}", escape(k)), out);
                    }
                }
            }
            Value::Array(a) => {
                for (i, v) in a.iter().enumerate() {
                    visit(v, &format!("{p}/{i}"), out)
                }
            }
            _ => {}
        }
    }
    let mut out = vec![];
    visit(v, "", &mut out);
    out
}
pub fn editable_paths(v: &Value) -> Vec<String> {
    let mut out = PARAMETERS
        .iter()
        .filter(|p| p.telegram_editable)
        .map(|p| format!("/parameters/{}/value", p.name))
        .collect::<Vec<_>>();
    out.extend(comment_paths(v));
    out
}
pub fn describe(c: &crate::model::Strategy) -> Result<Value, String> {
    crate::validate(c)?;
    let v = serde_json::to_value(c).map_err(|_| "serialization")?;
    fn leaves(v: &Value, p: &str, out: &mut Vec<Value>) {
        match v {
 Value::Object(m)=>for (k,v) in m {leaves(v,&format!("{p}/{}",escape(k)),out)},
 Value::Array(a)=>for (i,v) in a.iter().enumerate(){leaves(v,&format!("{p}/{i}"),out)},
 _=>out.push(json!({"path":p,"value":if p.starts_with("/messages/"){Value::Null}else{v.clone()},"telegram_editable":false})),}
    }
    let mut fields = vec![];
    leaves(&v, "", &mut fields);
    for field in &mut fields {
        if let Some(p) = PARAMETERS
            .iter()
            .find(|s| field["path"] == format!("/parameters/{}/value", s.name))
        {
            field["constraints"] = serde_json::to_value(p).unwrap();
            field["telegram_editable"] = json!(p.telegram_editable);
        } else if !field["path"].as_str().unwrap().starts_with("/messages/")
            && (field["path"].as_str().unwrap().starts_with("/policies/")
                || field["path"].as_str().unwrap().starts_with("/candles/")
                || field["path"] == "/instrument_price_tick_scale")
        {
            field["allowed_values"] = json!([field["value"].clone()]);
        }
    }
    Ok(
        json!({"schema_version":2,"engine_implemented":true,"fields":fields,"parameters":PARAMETERS,"comment_paths":comment_paths(&v),"disabled_modules":v["policies"]["modules"],"limitations":["risk limit unsupported; max_open_risk_r must be null","daily_point_a, weak_correction and range_trading unsupported","plan_days_statuses is editable in the file, not through Telegram"]}),
    )
}
