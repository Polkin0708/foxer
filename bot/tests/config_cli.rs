use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    process::{Command, Output},
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Workspace(PathBuf);
impl Workspace {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "forex-config-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(p.join("strategy")).unwrap();
        Self(p)
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_forex-bot"))
            .args(args)
            .current_dir(&self.0)
            .output()
            .unwrap()
    }
    fn write(&self, value: &Value) {
        fs::write(
            self.0.join("strategy/fvg-bot-rules.json"),
            serde_json::to_vec(value).unwrap(),
        )
        .unwrap();
    }
}
impl Drop for Workspace {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn working() -> Value {
    serde_json::from_str(include_str!("../../strategy/fvg-bot-rules.json")).unwrap()
}
fn rejection(value: Value, category: &str) {
    let w = Workspace::new();
    w.write(&value);
    let before = fs::read(w.0.join("strategy/fvg-bot-rules.json")).unwrap();
    let out = w.run(&["check-config"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(category),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read(w.0.join("strategy/fvg-bot-rules.json")).unwrap(),
        before
    );
}
#[test]
fn reads_working_spec_including_gap() {
    let w = Workspace::new();
    w.write(&working());
    let out = w.run(&["check-config"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "Configuration contract OK."
    );
}
#[test]
fn explicit_path_is_used() {
    let w = Workspace::new();
    fs::write(w.0.join("custom.json"), working().to_string()).unwrap();
    assert!(w.run(&["check-config", "custom.json"]).status.success());
}
#[test]
fn published_example_is_a_valid_strategy() {
    let w = Workspace::new();
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../example.json");
    let out = w.run(&["check-config", path.to_str().unwrap()]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}
#[test]
fn accepts_changed_numeric_parameters() {
    let mut v = working();
    v["parameters"]["plan_days"]["value"] = json!(12);
    v["parameters"]["stop_buffer_atr"]["value"] = json!(0);
    v["parameters"]["min_fvg_atr"]["value"] = json!(0.08);
    let w = Workspace::new();
    w.write(&v);
    assert!(w.run(&["check-config"]).status.success());
}
#[test]
fn missing_working_file_never_falls_back_to_example() {
    let w = Workspace::new();
    fs::write(w.0.join("example.json"), working().to_string()).unwrap();
    let out = w.run(&["check-config"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&out.stderr).contains("read"));
}
#[test]
fn incomplete_spec_is_not_a_working_config() {
    rejection(json!({"schema_version": 2}), "structure");
}
#[test]
fn malformed_and_duplicate_json_are_rejected_without_echoing_values() {
    for content in [
        "{\"secret\":\"DO_NOT_ECHO\",",
        "{\"schema_version\":1,\"schema_version\":1}",
    ] {
        let w = Workspace::new();
        fs::write(w.0.join("strategy/fvg-bot-rules.json"), content).unwrap();
        let out = w.run(&["check-config"]);
        assert_eq!(out.status.code(), Some(1));
        let error = String::from_utf8_lossy(&out.stderr);
        assert!(error.contains("structure"));
        assert!(!error.contains("DO_NOT_ECHO"));
    }
}
#[test]
fn rejects_unknown_fields_and_comment_types_at_all_depths() {
    for path in [
        "",
        "/policies/zones",
        "/policies/execution",
        "/parameters/plan_days",
        "/candles/weekly_boundary",
        "/instruments/0",
    ] {
        let mut v = working();
        v.pointer_mut(path).unwrap()["unknown"] = json!(true);
        rejection(v, "structure");
        let mut v = working();
        v.pointer_mut(path).unwrap()["comment"] = json!(42);
        rejection(v, "structure");
    }
}
#[test]
fn rejects_missing_fields_and_wrong_types() {
    for path in [
        "/candles/only_closed_bars",
        "/parameters/plan_days/value",
        "/policies/zones/gap",
        "/parameters/rr_min/value",
    ] {
        let mut v = working();
        *v.pointer_mut(path).unwrap() = json!(["wrong"]);
        rejection(v, "structure");
    }
    for field in ["gap", "fvg"] {
        let mut v = working();
        v["policies"]["zones"]
            .as_object_mut()
            .unwrap()
            .remove(field);
        rejection(v, "structure");
    }
    let mut v = working();
    v["parameters"]["rr_min"]
        .as_object_mut()
        .unwrap()
        .remove("value");
    rejection(v, "structure");
}
#[test]
fn rejects_old_future_and_unsupported_policies() {
    for version in [1, 3] {
        let mut v = working();
        v["schema_version"] = json!(version);
        rejection(v, "expected 2");
    }
    for (path, value, category) in [
        (
            "/policies/execution/market_quote",
            json!("last_close"),
            "structure",
        ),
        (
            "/policies/execution/entry_mode",
            json!("unsupported_mode"),
            "structure",
        ),
        (
            "/policies/modules/range_trading",
            json!(true),
            "unsupported",
        ),
        ("/candles/timezone", json!("UTC"), "unsupported"),
        (
            "/policies/targets/weekly_poi_daily_alternative/minimum_rr",
            json!("rr_max_trend"),
            "structure",
        ),
        (
            "/policies/plans/daily_poi",
            json!("weekly_zone"),
            "structure",
        ),
    ] {
        let mut v = working();
        *v.pointer_mut(path).unwrap() = value;
        rejection(v, category);
    }
}
#[test]
fn rejects_invalid_ranges_and_inconsistent_context() {
    for (path, value) in [
        ("/parameters/plan_days/value", json!(0)),
        ("/parameters/atr_period/value", json!(0)),
        ("/parameters/stop_buffer_atr/value", json!(-0.1)),
        ("/parameters/rr_min/value", json!(0)),
        ("/parameters/rr_max_trend/value", json!(1)),
        ("/context/minimum_bars/W", json!(0)),
    ] {
        let mut v = working();
        *v.pointer_mut(path).unwrap() = value;
        rejection(v, "range");
    }
    for key in [
        "stop_fallback_h4_bars",
        "confirmation_inversion_h4_bars",
        "weekly_fractal_sweep_lookback_bars",
        "target_fractal_lookback_d_bars",
        "target_fractal_lookback_w_bars",
    ] {
        let mut v = working();
        v["parameters"][key]["value"] = json!(0);
        rejection(v, "range");
        let mut v = working();
        v["parameters"][key]["value"] = json!(1.5);
        rejection(v, "structure");
    }
    let mut v = working();
    v["context"]["bars_analyzed"]["W"] = json!(1);
    rejection(v, "inconsistent");
    let mut v = working();
    v["parameters"]["rr_min"]["value"] = json!(11);
    rejection(v, "range");
}
#[test]
fn rejects_unknown_duplicate_and_missing_rules_and_variants() {
    for path in [
        "/policies/plans/daily_rules",
        "/policies/plans/weekly_rules",
        "/policies/plans/confirmation",
        "/policies/plans/entry",
        "/policies/execution/pending_cancel_on",
    ] {
        let mut v = working();
        v.pointer_mut(path).unwrap()[0] = json!("invented");
        rejection(v, "structure");
        let mut v = working();
        let a = v.pointer_mut(path).unwrap().as_array_mut().unwrap();
        a.push(a[0].clone());
        rejection(v, "rule");
        let mut v = working();
        v.pointer_mut(path).unwrap().as_array_mut().unwrap().pop();
        rejection(v, "rule");
    }
}
#[test]
fn nested_duplicates_and_duplicate_comments_are_rejected() {
    for content in [
        "{\"schema_version\":2,\"parameters\":{\"x\":1,\"x\":2}}",
        "{\"schema_version\":2,\"comment\":\"a\",\"comment\":\"b\"}",
    ] {
        let w = Workspace::new();
        fs::write(w.0.join("strategy/fvg-bot-rules.json"), content).unwrap();
        assert!(!w.run(&["check-config"]).status.success());
    }
}
#[test]
fn usage_errors_do_not_start_service() {
    let w = Workspace::new();
    for args in [vec![], vec!["run"], vec!["check-config", "one", "two"]] {
        assert_eq!(w.run(&args).status.code(), Some(2));
    }
}
fn describe(v: &Value) -> Value {
    let w = Workspace::new();
    w.write(v);
    let before = fs::read(w.0.join("strategy/fvg-bot-rules.json")).unwrap();
    let out = w.run(&["describe-config"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        fs::read(w.0.join("strategy/fvg-bot-rules.json")).unwrap(),
        before
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
#[test]
fn describe_covers_values_constraints_comments_and_disabled_modules_without_echoing_prose() {
    let mut v = working();
    v["comment"] = json!("PRIVATE_TEXT");
    v["parameters"]["plan_days"]["value"] = json!(17);
    let r = describe(&v);
    assert_eq!(r["schema_version"], 2);
    assert!(!r.to_string().contains("PRIVATE_TEXT"));
    assert_eq!(r["disabled_modules"]["range_trading"], false);
    let fields = r["fields"].as_array().unwrap();
    let p = fields
        .iter()
        .find(|f| f["path"] == "/parameters/plan_days/value")
        .unwrap();
    assert_eq!(p["value"], 17);
    assert_eq!(p["telegram_editable"], true);
    assert_eq!(p["constraints"]["minimum"], 1.0);
    fn leaves(v: &Value, path: String, out: &mut Vec<String>) {
        match v {
            Value::Object(m) => {
                for (k, v) in m {
                    if k != "comment" {
                        leaves(v, format!("{path}/{k}"), out)
                    }
                }
            }
            Value::Array(a) => {
                for (i, v) in a.iter().enumerate() {
                    leaves(v, format!("{path}/{i}"), out)
                }
            }
            _ => out.push(path),
        }
    }
    let mut paths = vec![];
    leaves(&v, String::new(), &mut paths);
    for path in paths {
        assert!(fields.iter().any(|f| f["path"] == path), "{path}");
    }
    for f in fields {
        assert!(v.pointer(f["path"].as_str().unwrap()).is_some());
    }
    assert!(r["comment_paths"]
        .as_array()
        .unwrap()
        .contains(&json!("/parameters/plan_days/comment")));
}
#[test]
fn rule_order_does_not_change_calculation_configuration() {
    let mut v = working();
    let before = forex_core::document::calculations(&v).unwrap();
    for key in ["daily_rules", "weekly_rules"] {
        v["policies"]["plans"][key]
            .as_array_mut()
            .unwrap()
            .reverse();
    }
    assert_eq!(forex_core::document::calculations(&v).unwrap(), before);
    describe(&v);
}
#[test]
fn describe_explicit_path_and_invalid_config() {
    let w = Workspace::new();
    fs::write(w.0.join("custom.json"), working().to_string()).unwrap();
    assert!(w.run(&["describe-config", "custom.json"]).status.success());
    let mut v = working();
    v["schema_version"] = json!(99);
    w.write(&v);
    let out = w.run(&["describe-config"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
}
