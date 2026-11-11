use forex_bot::{
    engine::Timeframe,
    feed::{Aggregator, Tick},
    trading::*,
};
use serde_json::json;
fn config() -> forex_bot::model::Strategy {
    let mut v: serde_json::Value =
        serde_json::from_str(include_str!("../../strategy/fvg-bot-rules.json")).unwrap();
    v["context"]
        .as_object_mut()
        .unwrap()
        .remove("historical_trade_replay");
    let c = serde_json::from_value(v).expect("agreed-v4 must deserialize");
    forex_bot::validate(&c).expect("agreed-v4 must validate");
    c
}
#[test]
fn v4_contract_accepts_new_fields() {
    let _ = config();
}
#[test]
fn summer_h4_closes_at_new_york_boundary() {
    let start = forex_bot::data::utc("2026-07-06T21:00:00Z").unwrap();
    let mut a = Aggregator::default();
    let tick = |t| Tick {
        symbol: "EURUSD".into(),
        millis: t * 1000,
        bid: 100,
        ask: 101,
        source: "test".into(),
    };
    a.push(tick(start)).unwrap();
    let bars = a.push(tick(start + 14400)).unwrap();
    assert!(bars
        .iter()
        .any(|(tf, b)| *tf == Timeframe::H4 && b.end == start + 14400));
}
#[test]
fn fractional_rr_is_not_truncated_and_rounds_protectively() {
    let mut c = config();
    c.parameters.rr_min.value = 2.5;
    let s = Signal::new(
        "long",
        Side::Long,
        Quote {
            time: 1,
            bid: 110,
            ask: 111,
        },
        100,
        135,
        1,
        &c,
    )
    .unwrap();
    assert_eq!((s.entry, s.state), (110, SignalState::Pending));
    let s = Signal::new(
        "short",
        Side::Short,
        Quote {
            time: 1,
            bid: 109,
            ask: 110,
        },
        120,
        85,
        1,
        &c,
    )
    .unwrap();
    assert_eq!((s.entry, s.state), (110, SignalState::Pending));
}
use forex_bot::engine::Engine;
fn bar(end: i64, open: i64, high: i64, low: i64, close: i64) -> Candle {
    Candle {
        end,
        open,
        high,
        low,
        close,
    }
}
fn small_config() -> forex_bot::model::Strategy {
    let mut c = config();
    c.parameters.atr_period.value = 1;
    c.context.minimum_bars.h4 = 1;
    c.context.minimum_bars.d = 1;
    c.context.minimum_bars.w = 1;
    c
}
fn pending_engine(side: Side, ifvg: bool) -> Engine {
    let c = small_config();
    let mut e = Engine::default();
    e.add_plan(
        "D_correction_end",
        side,
        Timeframe::D,
        Zone::new(side, 90, 100, 1, ZoneKind::Fvg),
        10,
    );
    let mut v = serde_json::to_value(e).unwrap();
    let s = Signal::new(
        "pending",
        side,
        Quote {
            time: 10,
            bid: 110,
            ask: 111,
        },
        if side == Side::Long { 90 } else { 130 },
        if side == Side::Long { 120 } else { 100 },
        1,
        &c,
    )
    .unwrap();
    assert_eq!(s.state, SignalState::Pending);
    v["signals"] = json!([s]);
    v["plans"][0]["signal"] = json!("pending");
    v["plans"][0]["state"] = json!("Setup");
    v["plans"][0]["entry_zone"] = json!(Zone::new(
        if side == Side::Long {
            Side::Short
        } else {
            Side::Long
        },
        100,
        110,
        2,
        ZoneKind::Fvg
    ));
    v["plans"][0]["entry_is_ifvg"] = json!(ifvg);
    v["plans"][0]["entry_atr"] = json!(20.0);
    serde_json::from_value(v).unwrap()
}
#[test]
fn expiry_precedes_fill_and_survives_restart() {
    let c = small_config();
    let mut e = pending_engine(Side::Long, false);
    let t = 10 + c.parameters.plan_days.value as i64 * 86400;
    e.quote(
        Quote {
            time: t,
            bid: 99,
            ask: 100,
        },
        1,
        &c,
    )
    .unwrap();
    assert_eq!(e.signals()[0].state, SignalState::Cancelled);
    let mut e: Engine = serde_json::from_value(serde_json::to_value(e).unwrap()).unwrap();
    e.quote(
        Quote {
            time: t + 1,
            bid: 99,
            ask: 100,
        },
        1,
        &c,
    )
    .unwrap();
    assert_eq!(e.signals()[0].state, SignalState::Cancelled);
}
#[test]
fn ifvg_cancellation_is_directional_inclusive_and_only_for_ifvg() {
    for side in [Side::Long, Side::Short] {
        for inverse in [false, true] {
            let c = small_config();
            let mut e = pending_engine(side, inverse);
            let close = if side == Side::Long { 98 } else { 112 };
            e.closed(Timeframe::H4, bar(11, 105, 115, 95, close), &c)
                .unwrap();
            assert_eq!(
                e.signals()[0].state,
                if inverse {
                    SignalState::Cancelled
                } else {
                    SignalState::Pending
                }
            );
        }
    }
}
#[test]
fn formation_atr_controls_inversion_instead_of_current_volatility() {
    for (reference, expected) in [
        ("formation_bar", ZoneState::Inverted),
        ("last_closed_bar", ZoneState::Tested),
    ] {
        let mut c = small_config();
        c.parameters.atr_reference_zone.value = reference.into();
        let mut e = Engine::default();
        e.seed(Timeframe::H4, bar(1, 105, 110, 100, 105), &c)
            .unwrap();
        let mut v = serde_json::to_value(e).unwrap();
        v["frames"]["H4"]["zones"] =
            json!([{ "zone":Zone::new(Side::Long,100,110,1,ZoneKind::Fvg),"atr":10.0}]);
        let mut e: Engine = serde_json::from_value(v).unwrap();
        e.seed(Timeframe::H4, bar(2, 105, 200, 98, 98), &c).unwrap();
        assert_eq!(
            serde_json::to_value(e).unwrap()["frames"]["H4"]["zones"][0]["zone"]["state"],
            json!(expected)
        );
    }
}
#[test]
fn stale_revision_cannot_change_published_levels() {
    let c = small_config();
    let mut e = pending_engine(Side::Long, false);
    let before = e.signals()[0].clone();
    let mut changed = before.clone();
    changed.stop = 95;
    assert!(!e.activate_revision(&changed).unwrap());
    assert!(e.reconfigure(&c, 1).unwrap().is_empty());
    assert_eq!(e.signals()[0].stop, before.stop);
}
#[test]
fn unsupported_modes_and_invalid_references_are_rejected() {
    for (path, value) in [
        ("/policies/risk/max_open_risk_r", json!(2)),
        (
            "/policies/targets/weekly_poi_daily_alternative/minimum_rr",
            json!("parameters.rr_max_trend.value"),
        ),
    ] {
        let mut v = serde_json::to_value(config()).unwrap();
        *v.pointer_mut(path).unwrap() = value;
        assert!(serde_json::from_value(v)
            .map_err(|e| e.to_string())
            .and_then(|c| forex_bot::validate(&c))
            .is_err());
    }
}
#[test]
fn legacy_snapshot_drops_old_grid_but_keeps_signal_and_comments() {
    use forex_bot::{
        runtime::Runtime,
        state::{Kind, Store},
    };
    let p = std::env::temp_dir().join(format!("v4-migration-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&p);
    let rules = serde_json::to_value(config()).unwrap();
    let mut e = pending_engine(Side::Long, true);
    e.seed(Timeframe::H4, bar(1, 105, 110, 100, 105), &small_config())
        .unwrap();
    e.add_plan(
        "W_point_A",
        Side::Long,
        Timeframe::W,
        Zone::new(Side::Long, 90, 100, 2, ZoneKind::Fvg),
        10,
    );
    let mut r = Runtime::open(&p, rules.clone(), e).unwrap();
    r.link(9, "pending").unwrap();
    r.update(&json!({"update_id":1,"message":{"message_id":10,"chat":{"id":-1},"from":{"id":1},"text":"question","reply_to_message":{"message_id":9}}}),-1).unwrap();
    drop(r);
    let mut store = Store::open(&p).unwrap();
    let mut v = store.entities(Kind::Runtime).unwrap().remove(0).value;
    v.as_object_mut().unwrap().remove("calendar_version");
    store.save_runtime(&v).unwrap();
    drop(store);
    let r = Runtime::open(&p, rules.clone(), Engine::default()).unwrap();
    assert_eq!(r.next_comment().unwrap().update_id, 1);
    assert!(r.pending().unwrap().is_empty());
    drop(r);
    let store = Store::open(&p).unwrap();
    let v = store.entities(Kind::Runtime).unwrap().remove(0).value;
    assert_eq!(v["calendar_version"], 4);
    assert_eq!(
        v["markets"]["EURUSD"]["engine"]["plans"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(v["markets"]["EURUSD"]["engine"]["frames"]
        .as_object()
        .unwrap()
        .is_empty());
    assert_eq!(v["markets"]["EURUSD"]["engine"]["signals"][0]["stop"], 90);
    drop(store);
    let _ = Runtime::open(&p, rules, Engine::default()).unwrap();
    let store = Store::open(&p).unwrap();
    assert_eq!(store.entities(Kind::Runtime).unwrap().remove(0).value, v);
}
#[test]
fn calendar_dst_weeks_and_entry_windows() {
    use forex_bot::{
        calendar::{bounds, entry_window},
        data::utc,
    };
    for (time, start, end) in [
        (
            "2026-03-04T12:00:00Z",
            "2026-03-01T22:00:00Z",
            "2026-03-08T21:00:00Z",
        ),
        (
            "2026-10-28T12:00:00Z",
            "2026-10-25T21:00:00Z",
            "2026-11-01T22:00:00Z",
        ),
    ] {
        assert_eq!(
            bounds(utc(time).unwrap(), Timeframe::W).unwrap(),
            (utc(start).unwrap(), utc(end).unwrap())
        );
    }
    let start = utc("2026-03-08T21:00:00Z").unwrap();
    assert_eq!(
        entry_window(start, "plan_day_open").unwrap(),
        utc("2026-03-07T22:00:00Z").unwrap()
    );
    assert_eq!(
        entry_window(start, "plan_start_minus_24h").unwrap(),
        start - 86400
    );
}
fn ready_ifvg(side: Side, close: i64) -> Engine {
    let c = small_config();
    let mut e = Engine::default();
    let mirror = |v| if side == Side::Long { v } else { 300 - v };
    for t in 1..=10 {
        e.seed(Timeframe::H4, bar(t, 100, 120, 80, 100), &c)
            .unwrap();
    }
    for (t, h, l) in [(1, 160, 150), (2, 170, 140), (3, 160, 150)] {
        e.seed(Timeframe::D, bar(t, 155, h, l, 155), &c).unwrap();
    }
    e.add_plan(
        "D_correction_end",
        side,
        Timeframe::D,
        Zone::new(side, 90, 100, 1, ZoneKind::Fvg),
        10,
    );
    let mut v = serde_json::to_value(e).unwrap();
    v["plans"][0]["state"] = json!("Ready");
    v["plans"][0]["entry_is_ifvg"] = json!(true);
    v["plans"][0]["entry_atr"] = json!(20.0);
    v["plans"][0]["entry_close"] = json!(mirror(close));
    let (lo, hi) = if side == Side::Long {
        (100, 110)
    } else {
        (190, 200)
    };
    v["plans"][0]["entry_zone"] = json!(Zone::new(
        if side == Side::Long {
            Side::Short
        } else {
            Side::Long
        },
        lo,
        hi,
        2,
        ZoneKind::Fvg
    ));
    // Independent structural target and protective stop fixtures, mirrored for short.
    v["frames"]["D"]["fractals"] =
        json!([{ "price":mirror(130),"upper":side==Side::Long,"confirmed_at":3,"swept":false}]);
    v["frames"]["D"]["zones"] = json!([]);
    v["frames"]["H4"]["fractals"] =
        json!([{ "price":mirror(90),"upper":side==Side::Short,"confirmed_at":9,"swept":false}]);
    serde_json::from_value(v).unwrap()
}
#[test]
fn ifvg_limit_checks_final_price_and_all_range_edges_for_both_sides() {
    for side in [Side::Long, Side::Short] {
        let mirror = |v| if side == Side::Long { v } else { 300 - v };
        for (target, close, policy, allowed) in [
            (130, 120, "inside_zone", true),
            (150, 120, "inside_zone", true),
            (153, 120, "inside_zone", false),
            (153, 120, "close_to_far_edge", true),
            (180, 120, "close_to_far_edge", true),
            (183, 120, "close_to_far_edge", false),
            (117, 120, "inside_zone", false),
        ] {
            let mut c = small_config();
            c.parameters.stop_buffer_atr.value = 0.0;
            c.parameters.ifvg_limit_range.value = policy.into();
            let mut v = serde_json::to_value(ready_ifvg(side, close)).unwrap();
            v["frames"]["D"]["fractals"][0]["price"] = json!(mirror(target));
            let mut e: Engine = serde_json::from_value(v).unwrap();
            let bid = if side == Side::Long {
                mirror(target) - 2
            } else {
                mirror(target) + 1
            };
            e.quote(
                Quote {
                    time: 11,
                    bid,
                    ask: bid + 1,
                },
                1,
                &c,
            )
            .unwrap();
            assert_eq!(
                !e.signals().is_empty(),
                allowed,
                "{side:?} {target} {policy}"
            );
        }
    }
}
#[test]
fn different_plan_kinds_use_their_own_fractional_caps() {
    let mut c = small_config();
    c.parameters.rr_max_trend.value = 3.5;
    c.parameters.rr_max_correction.value = 2.5;
    c.parameters.stop_buffer_atr.value = 0.0;
    for (rule, kind, expected) in [
        ("D_correction_end", "continuation", 153),
        ("D_correction_start", "correction", 139),
    ] {
        let mut v = serde_json::to_value(ready_ifvg(Side::Long, 120)).unwrap();
        v["plans"][0]["rule"] = json!(rule);
        v["plans"][0]["kind"] = json!(kind);
        v["plans"][0]["entry_is_ifvg"] = json!(false);
        v["frames"]["D"]["fractals"][0]["price"] = json!(200);
        let mut e: Engine = serde_json::from_value(v).unwrap();
        e.quote(
            Quote {
                time: 11,
                bid: 103,
                ask: 104,
            },
            1,
            &c,
        )
        .unwrap();
        assert_eq!(e.signals()[0].take_profit, expected);
    }
}
#[test]
fn ifvg_switch_disables_entry_but_preserves_confirmation() {
    let mut c = small_config();
    c.parameters.ifvg_entry.value = false;
    let mut e = Engine::default();
    e.seed(Timeframe::H4, bar(1, 105, 110, 100, 105), &c)
        .unwrap();
    e.add_plan(
        "W_point_A",
        Side::Long,
        Timeframe::W,
        Zone::new(Side::Long, 80, 90, 1, ZoneKind::Fvg),
        1,
    );
    let mut v = serde_json::to_value(e).unwrap();
    v["frames"]["H4"]["zones"] =
        json!([{ "zone":Zone::new(Side::Short,100,110,1,ZoneKind::Fvg),"atr":10.0}]);
    let mut e: Engine = serde_json::from_value(v).unwrap();
    e.closed(Timeframe::H4, bar(2, 105, 115, 100, 112), &c)
        .unwrap();
    assert_eq!(
        serde_json::to_value(e).unwrap()["plans"][0]["state"],
        "WaitEntry"
    );
}
#[test]
fn published_pending_levels_survive_reconfiguration_and_snapshot() {
    let mut c = small_config();
    let mut e = pending_engine(Side::Short, true);
    let before = serde_json::to_value(e.signals()).unwrap();
    c.parameters.stop_buffer_atr.value = 0.8;
    c.parameters.rr_min.value = 3.0;
    assert!(e.reconfigure(&c, 1).unwrap().is_empty());
    let e: Engine = serde_json::from_value(serde_json::to_value(e).unwrap()).unwrap();
    assert_eq!(serde_json::to_value(e.signals()).unwrap(), before);
}
#[test]
fn ifvg_age_uses_selected_day_window_on_dst_change() {
    use forex_bot::data::utc;
    let started = utc("2026-03-08T21:00:00Z").unwrap();
    let formed = started - 86400 + 1800;
    for (policy, expected) in [
        ("plan_day_open", "WaitEntry"),
        ("plan_start_minus_24h", "Ready"),
    ] {
        let mut c = small_config();
        c.parameters.ifvg_max_zone_age.value = policy.into();
        let mut e = Engine::default();
        e.seed(Timeframe::H4, bar(formed, 105, 110, 100, 105), &c)
            .unwrap();
        e.add_plan(
            "D_correction_end",
            Side::Long,
            Timeframe::D,
            Zone::new(Side::Long, 80, 90, 1, ZoneKind::Fvg),
            started,
        );
        let mut v = serde_json::to_value(e).unwrap();
        v["frames"]["H4"]["zones"] =
            json!([{ "zone":Zone::new(Side::Short,100,110,formed,ZoneKind::Fvg),"atr":10.0}]);
        let mut e: Engine = serde_json::from_value(v).unwrap();
        e.closed(Timeframe::H4, bar(started + 14400, 105, 115, 100, 112), &c)
            .unwrap();
        assert_eq!(
            serde_json::to_value(e).unwrap()["plans"][0]["state"],
            expected
        );
    }
}
#[test]
fn cancellation_does_not_trigger_inside_margin_and_zero_margin_includes_boundary() {
    let mut c = small_config();
    let mut e = pending_engine(Side::Long, true);
    e.closed(Timeframe::H4, bar(11, 105, 115, 99, 99), &c)
        .unwrap();
    assert_eq!(e.signals()[0].state, SignalState::Pending);
    c.parameters.inv_margin_atr.value = 0.0;
    e.closed(Timeframe::H4, bar(12, 105, 115, 100, 100), &c)
        .unwrap();
    assert_eq!(e.signals()[0].state, SignalState::Cancelled);
}
#[test]
fn stale_outbox_revisions_are_retired_without_delivery() {
    use forex_bot::{
        runtime::Runtime,
        state::{Outbound, Store},
    };
    let p = std::env::temp_dir().join(format!("v4-revisions-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&p);
    let rules = serde_json::to_value(config()).unwrap();
    let r = Runtime::open(&p, rules.clone(), pending_engine(Side::Long, false)).unwrap();
    drop(r);
    let mut store = Store::open(&p).unwrap();
    store
        .apply(
            "old",
            &json!({}),
            &[],
            &[Outbound {
                id: "revision:old".into(),
                payload: json!({"text":"old levels","revision":{},"signal_id":"pending"}),
            }],
        )
        .unwrap();
    drop(store);
    let mut r = Runtime::open(&p, rules, Engine::default()).unwrap();
    assert!(r.pending().unwrap().is_empty());
    r.delivered("revision:old", 99).unwrap();
    assert_eq!(r.signal_context("pending")["stop"], 90);
}
#[test]
fn failed_migration_can_retry_without_losing_legacy_state() {
    use forex_bot::{
        runtime::Runtime,
        state::{Kind, Store},
    };
    let p = std::env::temp_dir().join(format!("v4-retry-{}.sqlite", std::process::id()));
    let _ = std::fs::remove_file(&p);
    let rules = serde_json::to_value(config()).unwrap();
    drop(Runtime::open(&p, rules.clone(), pending_engine(Side::Long, false)).unwrap());
    let mut store = Store::open(&p).unwrap();
    let mut legacy = store.entities(Kind::Runtime).unwrap().remove(0).value;
    legacy.as_object_mut().unwrap().remove("calendar_version");
    store.save_runtime(&legacy).unwrap();
    drop(store);
    let db = rusqlite::Connection::open(&p).unwrap();
    db.execute(
        "INSERT INTO minutes(symbol,source,timestamp,payload) VALUES('EURUSD','test',1,'{}')",
        [],
    )
    .unwrap();
    assert!(Runtime::open(&p, rules.clone(), Engine::default()).is_err());
    let store = Store::open(&p).unwrap();
    assert_eq!(
        store.entities(Kind::Runtime).unwrap().remove(0).value,
        legacy
    );
    drop(store);
    db.execute("DELETE FROM minutes WHERE source='test'", [])
        .unwrap();
    drop(db);
    let r = Runtime::open(&p, rules, Engine::default()).unwrap();
    assert_eq!(r.signal_context("pending")["stop"], 90);
}

#[test]
fn removed_replay_field_is_rejected_as_unknown() {
    let mut v = serde_json::to_value(config()).unwrap();
    v["context"]["historical_trade_replay"] = json!(false);
    assert!(serde_json::from_value::<forex_bot::model::Strategy>(v).is_err());
}
