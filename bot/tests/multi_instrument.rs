use forex_bot::{
    engine::Engine,
    runtime::Runtime,
    state::{Kind, Store},
};
use serde_json::{json, Value};
fn rules() -> Value {
    let v: Value = serde_json::from_str(include_str!("../../strategy/fvg-bot-rules.json")).unwrap();
    v
}
fn path(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("multi-{}-{name}.sqlite", std::process::id()))
}
#[test]
fn restart_uses_supplied_json_not_database_rules() {
    let p = path("source");
    let r = Runtime::open(&p, rules(), Engine::default()).unwrap();
    drop(r);
    let mut updated = rules();
    updated["parameters"]["stop_buffer_atr"]["value"] = json!(0.7);
    let r = Runtime::open(&p, updated.clone(), Engine::default()).unwrap();
    assert_eq!(r.rules(), updated);
}
#[test]
fn snapshot_does_not_store_rules() {
    let p = path("no-rules");
    drop(Runtime::open(&p, rules(), Engine::default()).unwrap());
    let s = Store::open(&p).unwrap();
    assert!(s.entities(Kind::Runtime).unwrap()[0]
        .value
        .get("rules")
        .is_none());
}
#[test]
fn configuration_accepts_multiple_pairs() {
    let mut v = rules();
    v["instruments"] = json!([
 {"symbol":"EURUSD","price_tick":1},
 {"symbol":"GBPUSD","price_tick":1},
 {"symbol":"USDJPY","price_tick":1}]);
    let c = serde_json::from_value(v).unwrap();
    assert!(forex_bot::validate(&c).is_ok());
}
fn fixture_tick(symbol: &str, price: i64) -> forex_bot::feed::Tick {
    forex_bot::feed::Tick {
        symbol: symbol.into(),
        millis: 1_794_225_600_000,
        price,
        source: "upscale:index".into(),
    }
}
#[test]
fn aggregator_accepts_gbp_and_jpy_prices() {
    for (symbol, price) in [("GBPUSD", 1_250_000), ("USDJPY", 153_123_000)] {
        let mut a = forex_bot::feed::Aggregator::default();
        a.push(fixture_tick(symbol, price)).unwrap();
        let snapshot = serde_json::to_value(a).unwrap();
        assert_eq!(snapshot["last"]["symbol"], symbol);
        assert_eq!(snapshot["last"]["price"], price);
    }
}
#[test]
fn different_symbols_are_not_duplicate_quotes() {
    let p = path("isolation");
    let mut r = Runtime::open(&p, rules(), Engine::default()).unwrap();
    for (symbol, price) in [
        ("EURUSD", 1_100_000),
        ("GBPUSD", 1_300_000),
        ("USDJPY", 153_123_000),
    ] {
        let t = fixture_tick(symbol, price);
        r.tick(t, true).unwrap();
    }
    drop(r);
    let s = Store::open(&p).unwrap();
    let entries = s.entities(Kind::Runtime).unwrap();
    let markets = &entries[0].value["markets"];
    assert_eq!(markets.as_object().unwrap().len(), 3);
    assert_ne!(
        markets["EURUSD"]["aggregator"]["last"]["price"],
        markets["USDJPY"]["aggregator"]["last"]["price"]
    );
}
fn file_runtime(name: &str) -> (std::path::PathBuf, std::path::PathBuf, Runtime) {
    let db = path(name);
    let file = db.with_extension("json");
    std::fs::write(&file, rules().to_string()).unwrap();
    let mut r = Runtime::open_file(&db, &file, Engine::default()).unwrap();
    r.link(9, "EURUSD:s").unwrap();
    r.update(&json!({"update_id":1,"message":{"message_id":10,"chat":{"id":-1},"from":{"id":1},"text":"buffer","reply_to_message":{"message_id":9}}}),-1).unwrap();
    (db, file, r)
}
fn change() -> forex_bot::protocols::Analysis {
    forex_bot::protocols::Analysis {
        action: "apply".into(),
        explanation: "changed".into(),
        changes: vec![forex_bot::protocols::Change {
            path: "/parameters/stop_buffer_atr/value".into(),
            value: json!(0.7),
        }],
    }
}
#[test]
fn file_write_failure_keeps_rules_queue_and_outbox() {
    let (_db, file, mut r) = file_runtime("write-fail");
    let old = r.rules();
    std::fs::create_dir(file.with_extension("json.pending")).unwrap();
    assert!(r.analysis(1, &change(), &json!({})).is_err());
    assert_eq!(r.rules(), old);
    assert!(r.next_comment().is_some());
    assert!(r.pending().unwrap().is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&std::fs::read(file).unwrap()).unwrap(),
        old
    );
}
#[test]
fn applied_change_is_in_json_and_never_in_snapshot_or_event_patch() {
    let (db, file, mut r) = file_runtime("apply");
    r.analysis(1, &change(), &json!({})).unwrap();
    drop(r);
    let r = Runtime::open_file(&db, &file, Engine::default()).unwrap();
    assert_eq!(r.rules()["parameters"]["stop_buffer_atr"]["value"], 0.7);
    assert!(r.next_comment().is_none());
    let db = rusqlite::Connection::open(db).unwrap();
    let patch: String = db
        .query_row("SELECT payload FROM inbox WHERE id='analysis:1'", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert!(!patch.contains("changes"));
}
#[test]
fn corrupted_or_missing_file_never_falls_back_to_snapshot() {
    let (db, file, r) = file_runtime("invalid-file");
    drop(r);
    std::fs::write(&file, "broken").unwrap();
    assert!(Runtime::open_file(&db, &file, Engine::default()).is_err());
    std::fs::remove_file(&file).unwrap();
    assert!(Runtime::open_file(&db, &file, Engine::default()).is_err());
}
#[test]
fn interruption_before_file_write_retains_comment() {
    let (db, file, r) = file_runtime("before-file");
    drop(r);
    let mut store = Store::open(&db).unwrap();
    let mut snap = store.entities(Kind::Runtime).unwrap()[0].value.clone();
    use sha2::{Digest, Sha256};
    let current: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    snap["operation"] = json!({"id":1,"before":format!("{:x}",Sha256::digest(current.to_string().as_bytes())),"after":"not-written"});
    store.save_runtime(&snap).unwrap();
    drop(store);
    let r = Runtime::open_file(&db, &file, Engine::default()).unwrap();
    assert!(
        r.next_comment().is_some(),
        "unwritten change must remain queued"
    );
}
#[test]
fn interruption_after_file_write_uses_json_once_without_patch() {
    let (db, file, r) = file_runtime("after-file");
    let before = r.rules();
    drop(r);
    let mut after = before.clone();
    after["parameters"]["stop_buffer_atr"]["value"] = json!(0.7);
    use sha2::{Digest, Sha256};
    let hash = |v: &Value| format!("{:x}", Sha256::digest(v.to_string().as_bytes()));
    let mut store = Store::open(&db).unwrap();
    let mut snap = store.entities(Kind::Runtime).unwrap()[0].value.clone();
    snap["operation"] = json!({"id":1,"before":hash(&before),"after":hash(&after)});
    store.save_runtime(&snap).unwrap();
    drop(store);
    std::fs::write(&file, after.to_string()).unwrap();
    let r = Runtime::open_file(&db, &file, Engine::default()).unwrap();
    assert_eq!(r.rules(), after);
    assert!(r.next_comment().is_none());
    assert_eq!(r.pending().unwrap().len(), 1);
    drop(r);
    let r = Runtime::open_file(&db, &file, Engine::default()).unwrap();
    assert_eq!(r.pending().unwrap().len(), 1);
}
#[test]
fn previous_database_is_not_rewritten_on_open() {
    let p = path("previous-schema");
    let c = rusqlite::Connection::open(&p).unwrap();
    c.execute_batch("PRAGMA application_id=1180190513; PRAGMA user_version=3; CREATE TABLE minutes(payload TEXT); INSERT INTO minutes VALUES('original-bytes');").unwrap();
    drop(c);
    let before = std::fs::read(&p).unwrap();
    assert!(forex_bot::data::open(&p).is_err());
    assert_eq!(std::fs::read(&p).unwrap(), before);
}
#[test]
fn quotes_only_track_their_instrument_and_repeat_safely_after_restart() {
    use forex_bot::trading::{Quote, Side, Signal};
    let p = path("separate-signals");
    let cfg = serde_json::from_value(rules()).unwrap();
    let mut r = Runtime::open(&p, rules(), Engine::default()).unwrap();
    for symbol in ["EURUSD", "GBPUSD", "USDJPY"] {
        let signal = Signal::new(
            "same",
            Side::Long,
            Quote {
                time: 1,
                price: 10000,
            },
            9000,
            15000,
            1,
            &cfg,
        )
        .unwrap();
        let mut e = serde_json::to_value(Engine::default()).unwrap();
        e["signals"] = json!([signal]);
        r.seed_instrument(symbol, serde_json::from_value(e).unwrap())
            .unwrap();
        let t = forex_bot::feed::Tick {
            symbol: symbol.into(),
            millis: 2000,
            price: 10000,

            source: "upscale:index".into(),
        };
        r.tick(t, false).unwrap();
    }
    let t = forex_bot::feed::Tick {
        symbol: "USDJPY".into(),
        millis: 3000,
        price: 8900,

        source: "upscale:index".into(),
    };
    r.tick(t.clone(), false).unwrap();
    assert_eq!(r.signal_context("USDJPY:same")["state"], "Stop");
    assert_eq!(r.signal_context("EURUSD:same")["state"], "Active");
    assert_eq!(r.signal_context("GBPUSD:same")["state"], "Active");
    let n = r.pending().unwrap().len();
    drop(r);
    let mut r = Runtime::open(&p, rules(), Engine::default()).unwrap();
    r.tick(t, false).unwrap();
    assert_eq!(r.pending().unwrap().len(), n);
    assert!(r
        .pending()
        .unwrap()
        .iter()
        .any(|m| m.payload["text"].as_str().unwrap().contains("USD/JPY")));
}
#[test]
fn symbols_are_configurable_but_malformed_duplicates_and_bad_ticks_fail() {
    let mut v = rules();
    v["instruments"][0]["symbol"] = json!("AUDCAD");
    assert!(forex_bot::validate(&serde_json::from_value(v.clone()).unwrap()).is_ok());
    for bad in [json!("audcad"), json!("../ABC"), json!("AUD")] {
        let mut b = v.clone();
        b["instruments"][0]["symbol"] = bad;
        assert!(forex_bot::validate(&serde_json::from_value(b).unwrap()).is_err());
    }
    for tick in [0, -1] {
        let mut b = v.clone();
        b["instruments"][0]["price_tick"] = json!(tick);
        assert!(forex_bot::validate(&serde_json::from_value(b).unwrap()).is_err());
    }
    v["instruments"][1] = v["instruments"][0].clone();
    assert!(forex_bot::validate(&serde_json::from_value(v).unwrap()).is_err());
}
#[test]
fn telegram_rule_change_preserves_published_levels_for_all_pairs() {
    use forex_bot::{
        engine::Timeframe,
        trading::{Candle, Quote, Side, Zone, ZoneKind},
    };
    let db = path("all-rules");
    let file = db.with_extension("json");
    let mut v = rules();
    v["parameters"]["atr_period"]["value"] = json!(1);
    for tf in ["W", "D", "H4"] {
        v["context"]["minimum_bars"][tf] = json!(1);
    }
    std::fs::write(&file, v.to_string()).unwrap();
    let cfg = serde_json::from_value(v).unwrap();
    let mut r = Runtime::open_file(&db, &file, Engine::default()).unwrap();
    let mut originals = Vec::new();
    for (symbol, scale, tick) in [
        ("EURUSD", 10000, 1),
        ("GBPUSD", 10000, 10),
        ("USDJPY", 1000000, 1000),
    ] {
        let mut e = Engine::default();
        let bar = |end, o, h, l, c| Candle {
            end,
            open: o * scale,
            high: h * scale,
            low: l * scale,
            close: c * scale,
        };
        for b in [
            bar(1, 280, 300, 270, 280),
            bar(2, 280, 310, 270, 280),
            bar(3, 280, 300, 270, 280),
        ] {
            e.seed(Timeframe::D, b, &cfg).unwrap();
        }
        for t in 1..=10 {
            e.seed(Timeframe::H4, bar(t, 100, 110, 90, 100), &cfg)
                .unwrap();
        }
        e.add_plan(
            "D_correction_end",
            Side::Long,
            Timeframe::D,
            Zone::new(Side::Long, 90 * scale, 100 * scale, 1, ZoneKind::Fvg),
            10,
        );
        e.closed(Timeframe::H4, bar(11, 125, 140, 120, 130), &cfg)
            .unwrap();
        e.quote(
            Quote {
                time: 12,
                price: 130 * scale,
            },
            tick,
            &cfg,
        )
        .unwrap();
        e.namespace_signals(symbol);
        assert_eq!(e.signals().len(), 1);
        originals.push((e.signals()[0].id.clone(), e.signals()[0].stop, tick));
        r.seed_instrument(symbol, e).unwrap();
    }
    r.link(9, &originals[0].0).unwrap();
    r.update(&json!({"update_id":1,"message":{"message_id":10,"chat":{"id":-1},"from":{"id":1},"text":"buffer","reply_to_message":{"message_id":9}}}),-1).unwrap();
    r.analysis(1, &change(), &json!({})).unwrap();
    let out = r.pending().unwrap();
    let revisions: Vec<_> = out
        .iter()
        .filter(|m| m.payload.get("revision").is_some())
        .collect();
    assert!(revisions.is_empty());
    for (id, stop, _) in &originals {
        assert_eq!(r.signal_context(id)["stop"], *stop);
    }
    for m in out {
        r.delivered(&m.id, 100 + r.offset()).unwrap();
    }
    drop(r);
    let r = Runtime::open_file(&db, &file, Engine::default()).unwrap();
    for (id, stop, _) in originals {
        assert_eq!(r.signal_context(&id)["stop"], stop);
    }
}

fn display_signal(id: &str) -> forex_bot::trading::Signal {
    serde_json::from_value(json!({
        "id": id, "side": "Long", "state": "Active", "entry": 1100000,
        "stop": 1090000, "target": 1130000, "take_profit": 1130000,
        "last_quote_time": 1
    }))
    .unwrap()
}

#[test]
fn signal_message_uses_its_actual_pair() {
    for (symbol, label) in [
        ("EURUSD", "EUR/USD"),
        ("GBPUSD", "GBP/USD"),
        ("USDJPY", "USD/JPY"),
        ("AUDCAD", "AUD/CAD"),
    ] {
        let text =
            forex_bot::runtime::signal_text(&display_signal(&format!("{symbol}:setup"))).unwrap();
        assert!(text.starts_with(label));
    }
}

#[test]
fn signal_message_never_invents_eurusd_for_unknown_pair() {
    for id in [
        "setup",
        "",
        ":setup",
        "gbpusd:setup",
        "USD:setup",
        "АБВГДЕ:setup",
    ] {
        let text = forex_bot::runtime::signal_text(&display_signal(id));
        assert_eq!(
            text.unwrap_err(),
            "Не удалось определить пару сигнала: отсутствует или некорректен префикс ID",
            "{id:?}"
        );
    }
}
