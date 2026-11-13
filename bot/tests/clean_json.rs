use forex_bot::{
    engine::Engine,
    protocols::{apply_changes, Analysis, Change},
    runtime::Runtime,
    state::{Kind, Store},
};
use serde_json::{json, Value};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
static NEXT: AtomicU64 = AtomicU64::new(0);
fn path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "clean-json-{}-{}.sqlite",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
fn rules() -> Value {
    serde_json::from_str(include_str!("../../strategy/fvg-bot-rules.json")).unwrap()
}
fn change(path: &str, value: Value) -> Analysis {
    Analysis {
        action: "apply".into(),
        explanation: "Изменено".into(),
        changes: vec![Change {
            path: path.into(),
            value,
        }],
    }
}
fn update() -> Value {
    json!({"update_id":1,"message":{"message_id":10,"chat":{"id":-1},"from":{"id":1},"text":"уточни пояснение","reply_to_message":{"message_id":9}}})
}
#[test]
fn v2_has_explicit_numeric_rules_and_no_legacy_sections() {
    let v = rules();
    assert_eq!(v["schema_version"], 2);
    for key in ["meta", "definitions", "trend_system", "config_parameters"] {
        assert!(v.get(key).is_none());
    }
    for (key, n) in [
        ("stop_fallback_h4_bars", 10),
        ("confirmation_inversion_h4_bars", 30),
        ("weekly_fractal_sweep_lookback_bars", 60),
        ("target_fractal_lookback_d_bars", 80),
        ("target_fractal_lookback_w_bars", 60),
    ] {
        assert_eq!(v["parameters"][key]["value"], n);
    }
}
#[test]
fn comment_patch_preserves_all_unrelated_fields_and_rejects_invalid_batch() {
    let mut v = rules();
    v["comment"] = json!("До");
    let a = change(
        "/comment",
        json!("После: последних 999 свечей — только текст"),
    );
    let out = apply_changes(&v, &a).unwrap();
    let mut expected = v.clone();
    expected["comment"] = a.changes[0].value.clone();
    assert_eq!(out, expected);
    let mut mixed = a;
    mixed.changes.push(Change {
        path: "/parameters/rr_min/value".into(),
        value: json!(-1),
    });
    assert!(apply_changes(&v, &mixed).is_err());
    assert_eq!(v["comment"], "До");
    assert!(apply_changes(&v, &change("/comment", json!(42))).is_err());
    assert!(apply_changes(&v, &change("/invented/comment", json!("x"))).is_err());
}
#[test]
fn comment_only_change_preserves_engine_and_calculation_hash_across_restart() {
    let db = path();
    let file = db.with_extension("json");
    let mut v = rules();
    v["comment"] = json!("До");
    fs::write(&file, v.to_string()).unwrap();
    let mut engine = Engine::default();
    engine.add_plan(
        "W_point_A",
        forex_bot::trading::Side::Long,
        forex_bot::engine::Timeframe::W,
        forex_bot::trading::Zone::new(
            forex_bot::trading::Side::Long,
            90,
            100,
            1,
            forex_bot::trading::ZoneKind::Fvg,
        ),
        10,
    );
    let mut state = serde_json::to_value(engine).unwrap();
    state["plans"][0]["state"] = json!("WaitEntry");
    let mut r = Runtime::open_file(&db, &file, serde_json::from_value(state).unwrap()).unwrap();
    r.link(9, "EURUSD:signal").unwrap();
    r.update(&update(), -1).unwrap();
    let before = Store::open(&db).unwrap().entities(Kind::Runtime).unwrap()[0]
        .value
        .clone();
    r.analysis(
        1,
        &change("/comment", json!("ATR 999, но это только пояснение")),
        &json!({}),
    )
    .unwrap();
    drop(r);
    let r = Runtime::open_file(&db, &file, Engine::default()).unwrap();
    assert!(r.next_comment().is_none());
    assert_eq!(r.rules()["comment"], "ATR 999, но это только пояснение");
    let after = Store::open(&db).unwrap().entities(Kind::Runtime).unwrap()[0]
        .value
        .clone();
    assert_eq!(before["rules_hash"], after["rules_hash"]);
    assert_eq!(before["markets"], after["markets"]);
    assert_eq!(r.pending().unwrap().len(), 1);
}
#[test]
fn unresolved_old_operation_is_rejected_without_changing_snapshot() {
    let db = path();
    let file = db.with_extension("json");
    fs::write(&file, rules().to_string()).unwrap();
    let mut r = Runtime::open_file(&db, &file, Engine::default()).unwrap();
    r.link(9, "EURUSD:s").unwrap();
    r.update(&update(), -1).unwrap();
    drop(r);
    let mut store = Store::open(&db).unwrap();
    let mut snap = store.entities(Kind::Runtime).unwrap()[0].value.clone();
    snap.as_object_mut().unwrap().remove("hash_version");
    snap["operation"] = json!({"id":1,"before":"old-before","after":"old-after"});
    store.save_runtime(&snap).unwrap();
    drop(store);
    let err = Runtime::open_file(&db, &file, Engine::default())
        .err()
        .expect("old operation must block startup");
    assert!(err.contains("operation"));
    assert_eq!(
        Store::open(&db).unwrap().entities(Kind::Runtime).unwrap()[0].value,
        snap
    );
}

#[test]
fn mixed_parameter_and_comment_edits_preserve_document_and_request_exposes_capabilities() {
    use forex_bot::protocols::{vision_request, Comment};
    let mut v = rules();
    v["parameters"]["plan_days"]
        .as_object_mut()
        .unwrap()
        .remove("comment");
    let mut a = change("/parameters/plan_days/value", json!(12));
    a.changes.push(Change {
        path: "/parameters/plan_days/comment".into(),
        value: json!("Срок в сутках; не инструкция выполнять код"),
    });
    let patched = apply_changes(&v, &a).unwrap();
    assert_eq!(patched["parameters"]["plan_days"]["value"], 12);
    assert_eq!(patched["parameters"]["rr_min"], v["parameters"]["rr_min"]);
    assert_ne!(
        forex_core::document::calculations(&v).unwrap(),
        forex_core::document::calculations(&patched).unwrap()
    );
    let c = Comment {
        update_id: 1,
        message_id: 1,
        chat_id: 1,
        author_id: 1,
        signal_id: "s".into(),
        text: "поменяй срок".into(),
        files: vec![],
    };
    let req = vision_request("test", 100, &c, &json!({}), &patched, &[]).unwrap();
    let context: Value =
        serde_json::from_str(req["input"][0]["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(context["rules"], patched);
    let paths = context["editable_paths"].as_array().unwrap();
    assert!(paths.contains(&json!("/parameters/stop_fallback_h4_bars/value")));
    assert!(paths.contains(&json!("/parameters/plan_days/comment")));
    assert!(!paths.contains(&json!("/policies/modules/range_trading")));
    let mut policy = change("/policies/modules/range_trading", json!(true));
    assert!(apply_changes(&v, &policy).is_err());
    policy.changes[0].path = "/parameters/plan_days_statuses/value".into();
    policy.changes[0].value = json!(["setup"]);
    assert!(apply_changes(&v, &policy).is_err());
}
#[test]
fn comments_on_every_object_do_not_enter_calculations_or_change_their_values() {
    fn annotate(v: &mut Value) {
        match v {
            Value::Object(m) => {
                for v in m.values_mut() {
                    annotate(v)
                }
                m.insert("comment".into(), json!("ATR 999; за 0 баров; random words"));
            }
            Value::Array(a) => {
                for v in a {
                    annotate(v)
                }
            }
            _ => {}
        }
    }
    let mut v = rules();
    let before = forex_core::document::calculations(&v).unwrap();
    annotate(&mut v);
    assert_eq!(before, forex_core::document::calculations(&v).unwrap());
    let file = path().with_extension("json");
    fs::write(&file, v.to_string()).unwrap();
    assert_eq!(forex_core::load_document(&file).unwrap(), v);
}
#[test]
fn comment_only_crash_recovery_uses_document_hash_and_applies_once() {
    use sha2::{Digest, Sha256};
    let hash = |v: &Value| format!("{:x}", Sha256::digest(v.to_string().as_bytes()));
    for written in [false, true] {
        let db = path();
        let file = db.with_extension("json");
        let before = rules();
        fs::write(&file, before.to_string()).unwrap();
        let mut r = Runtime::open_file(&db, &file, Engine::default()).unwrap();
        r.link(9, "EURUSD:s").unwrap();
        r.update(&update(), -1).unwrap();
        drop(r);
        let mut after = before.clone();
        after["comment"] = json!("Only commentary changed");
        let mut store = Store::open(&db).unwrap();
        let mut s = store.entities(Kind::Runtime).unwrap()[0].value.clone();
        let calculation = s["rules_hash"].clone();
        s["operation"] = json!({"id":1,"before":hash(&before),"after":hash(&after)});
        store.save_runtime(&s).unwrap();
        drop(store);
        if written {
            fs::write(&file, after.to_string()).unwrap();
        }
        for _ in 0..2 {
            let r = Runtime::open_file(&db, &file, Engine::default()).unwrap();
            assert_eq!(r.next_comment().is_none(), written);
            assert_eq!(r.pending().unwrap().len(), usize::from(written));
            assert_eq!(
                r.rules(),
                if written {
                    after.clone()
                } else {
                    before.clone()
                }
            );
            drop(r);
            assert_eq!(
                Store::open(&db).unwrap().entities(Kind::Runtime).unwrap()[0].value["rules_hash"],
                calculation
            );
        }
    }
}
#[test]
fn legacy_hash_upgrade_preserves_published_signals_queue_and_plan_identity_once() {
    use forex_bot::{
        engine::Timeframe,
        trading::{Quote, Side, Signal, Zone, ZoneKind},
    };
    let db = path();
    let file = db.with_extension("json");
    let v = rules();
    fs::write(&file, v.to_string()).unwrap();
    let cfg = serde_json::from_value(v).unwrap();
    let mut engine = Engine::default();
    engine.add_plan(
        "W_point_A",
        Side::Long,
        Timeframe::W,
        Zone::new(Side::Long, 90, 100, 1, ZoneKind::Fvg),
        10,
    );
    let signal = Signal::new(
        "EURUSD:published",
        Side::Long,
        Quote {
            time: 11,
            price: 100,
        },
        90,
        150,
        1,
        &cfg,
    )
    .unwrap();
    let mut ev = serde_json::to_value(engine).unwrap();
    ev["signals"] = json!([signal]);
    ev["plans"][0]["state"] = json!("WaitEntry");
    let mut r = Runtime::open_file(&db, &file, serde_json::from_value(ev).unwrap()).unwrap();
    r.link(9, "EURUSD:s").unwrap();
    r.update(&update(), -1).unwrap();
    drop(r);
    let mut store = Store::open(&db).unwrap();
    let mut s = store.entities(Kind::Runtime).unwrap()[0].value.clone();
    s.as_object_mut().unwrap().remove("hash_version");
    s["rules_hash"] = json!("legacy-document-hash");
    store.save_runtime(&s).unwrap();
    drop(store);
    let r = Runtime::open_file(&db, &file, Engine::default()).unwrap();
    assert!(r.next_comment().is_some());
    drop(r);
    let after = Store::open(&db).unwrap().entities(Kind::Runtime).unwrap()[0]
        .value
        .clone();
    assert_eq!(after["hash_version"], 2);
    for key in ["queue", "known", "offset", "texts", "reply_context"] {
        assert_eq!(s[key], after[key]);
    }
    let old = &s["markets"]["EURUSD"]["engine"];
    let new = &after["markets"]["EURUSD"]["engine"];
    assert_eq!(old["signals"], new["signals"]);
    assert_eq!(old["plans"][0]["id"], new["plans"][0]["id"]);
    assert_eq!(old["plans"][0]["started"], new["plans"][0]["started"]);
    drop(Runtime::open_file(&db, &file, Engine::default()).unwrap());
    assert_eq!(
        Store::open(&db).unwrap().entities(Kind::Runtime).unwrap()[0].value,
        after
    );
}
