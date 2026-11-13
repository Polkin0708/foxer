use forex_bot::{
    engine::Engine,
    feed::{Aggregator, Tick},
    runtime::Runtime,
};
use serde_json::json;
#[test]
fn one_price_tick_builds_and_restores_candles() {
    let t: Tick = serde_json::from_value(json!({"symbol":"EURUSD","millis":1767358800000_i64,"price":1100000,"source":"upscale:index"})).expect("single index price must deserialize");
    let mut a = Aggregator::default();
    a.push(t.clone()).unwrap();
    let state = serde_json::to_value(&a).unwrap();
    assert_eq!(state["last"]["price"], 1100000);
    let mut restored: Aggregator = serde_json::from_value(state).unwrap();
    assert!(restored.push(t).unwrap().is_empty());
}
#[test]
fn clean_index_database_reopens_without_price_basis_marker() {
    let rules: serde_json::Value =
        serde_json::from_slice(&std::fs::read("../strategy/fvg-bot-rules.json").unwrap()).unwrap();
    let path = std::env::temp_dir().join(format!("index-contract-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&path);
    drop(
        Runtime::open(&path, rules.clone(), Engine::default())
            .expect("clean index rules must open"),
    );
    drop(Runtime::open(&path, rules, Engine::default()).expect("state must reopen"));
    let store = forex_bot::state::Store::open(&path).unwrap();
    let snapshot = store.entities(forex_bot::state::Kind::Runtime).unwrap();
    assert!(snapshot[0].value.get("price_basis").is_none());
    assert_eq!(snapshot[0].value["markets"].as_object().unwrap().len(), 3);
    drop(store);
    std::fs::remove_file(path).unwrap();
}
