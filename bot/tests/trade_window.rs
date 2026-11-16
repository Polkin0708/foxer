//! Entry-time filter: policies.trade_window.session_filter + entry_h4_close_hours.
use forex_bot::{engine::*, trading::*};
use serde_json::{json, Value};

// H4 closes on Wednesday 2024-01-10 (America/New_York is UTC-5 in January).
const NY_21: i64 = 1_704_852_000; // Tuesday 21:00 New York
const NY_01: i64 = NY_21 + 14_400; // 01:00
const NY_05: i64 = NY_01 + 14_400; // 05:00
const H4: i64 = 14_400;

fn load(edit: impl FnOnce(&mut Value)) -> Result<forex_bot::model::Strategy, String> {
    let mut v: Value =
        serde_json::from_slice(&std::fs::read("../strategy/fvg-bot-rules.json").unwrap()).unwrap();
    edit(&mut v);
    let p = std::env::temp_dir().join(format!(
        "trade-window-{}-{:?}.json",
        std::process::id(),
        std::thread::current().id()
    ));
    std::fs::write(&p, v.to_string()).unwrap();
    let r = forex_bot::load(&p);
    std::fs::remove_file(&p).unwrap();
    r
}
fn cfg(window: Option<Value>) -> forex_bot::model::Strategy {
    let mut c = load(|v| {
        if let Some(hours) = window {
            v["policies"]["trade_window"]["session_filter"] = json!(true);
            v["policies"]["trade_window"]["entry_h4_close_hours"] = hours;
        }
    })
    .expect("strategy with entry-time filter must load");
    c.parameters.atr_period.value = 1;
    c.context.minimum_bars.w = 1;
    c.context.minimum_bars.d = 1;
    c.context.minimum_bars.h4 = 1;
    c
}
fn bar(t: i64) -> Candle {
    Candle {
        end: t,
        open: 104,
        high: 106,
        low: 103,
        close: 105,
    }
}
/// Engine with H4 context, one waiting plan and one live long H4 zone formed after the plan start.
fn waiting(c: &forex_bot::model::Strategy, rule: &str, tf: Timeframe) -> Value {
    let mut e = Engine::default();
    for k in (1..=4).rev() {
        e.seed(Timeframe::H4, bar(NY_21 - k * H4), c).unwrap();
    }
    let started = NY_21 - 2 * H4;
    assert!(e.add_plan(
        rule,
        Side::Long,
        tf,
        Zone::new(Side::Long, 90, 100, started, ZoneKind::Fvg),
        started
    ));
    let mut snap = serde_json::to_value(&e).unwrap();
    snap["frames"]["H4"]["zones"] = json!([
        {"zone": Zone::new(Side::Long, 95, 100, NY_21 - H4, ZoneKind::Fvg), "atr": 20.0}
    ]);
    snap
}
fn state_after(snap: &Value, c: &forex_bot::model::Strategy, closes: &[i64]) -> String {
    let mut e: Engine = serde_json::from_value(snap.clone()).unwrap();
    for &t in closes {
        e.closed(Timeframe::H4, bar(t), c).unwrap();
    }
    serde_json::to_value(&e).unwrap()["plans"][0]["state"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn without_filter_entry_zone_is_selected_on_any_h4_close() {
    let c = cfg(None);
    let snap = waiting(&c, "D_continuation_from_weekly_zone", Timeframe::D);
    assert_eq!(state_after(&snap, &c, &[NY_21]), "Ready");
}

#[test]
fn filter_defers_entry_zone_to_the_next_allowed_h4_close() {
    let c = cfg(Some(json!([5, 9, 13, 17])));
    let snap = waiting(&c, "D_continuation_from_weekly_zone", Timeframe::D);
    assert_eq!(state_after(&snap, &c, &[NY_21]), "WaitEntry");
    assert_eq!(state_after(&snap, &c, &[NY_21, NY_01]), "WaitEntry");
    assert_eq!(state_after(&snap, &c, &[NY_21, NY_01, NY_05]), "Ready");
}

#[test]
fn filter_boundary_hour_is_allowed_only_when_listed() {
    let snap_cfg = cfg(Some(json!([1])));
    let snap = waiting(&snap_cfg, "D_continuation_from_weekly_zone", Timeframe::D);
    assert_eq!(state_after(&snap, &snap_cfg, &[NY_21]), "WaitEntry");
    assert_eq!(state_after(&snap, &snap_cfg, &[NY_21, NY_01]), "Ready");
}

#[test]
fn filter_does_not_delay_plan_confirmation() {
    let c = cfg(Some(json!([5, 9, 13, 17])));
    let mut snap = waiting(&c, "W_point_A", Timeframe::W);
    // A short zone inverted by the close confirms the long plan.
    snap["frames"]["H4"]["zones"] = json!([
        {"zone": Zone::new(Side::Short, 105, 110, NY_21 - H4, ZoneKind::Fvg), "atr": 20.0}
    ]);
    let mut e: Engine = serde_json::from_value(snap).unwrap();
    assert_eq!(
        serde_json::to_value(&e).unwrap()["plans"][0]["state"],
        "WaitConfirm"
    );
    e.closed(
        Timeframe::H4,
        Candle {
            end: NY_21,
            open: 100,
            high: 120,
            low: 90,
            close: 119,
        },
        &c,
    )
    .unwrap();
    assert_eq!(
        serde_json::to_value(&e).unwrap()["plans"][0]["state"],
        "WaitEntry"
    );
}

#[test]
fn schema_requires_consistent_filter_and_hours() {
    let hours = |filter: bool, hours: Option<Value>| {
        load(|v| {
            v["policies"]["trade_window"]["session_filter"] = json!(filter);
            match hours {
                Some(h) => v["policies"]["trade_window"]["entry_h4_close_hours"] = h,
                None => {
                    v["policies"]["trade_window"]
                        .as_object_mut()
                        .unwrap()
                        .remove("entry_h4_close_hours");
                }
            }
        })
    };
    assert!(hours(false, None).is_ok());
    assert!(hours(false, Some(json!([]))).is_ok());
    assert!(hours(true, Some(json!([5, 9, 13, 17]))).is_ok());
    for (filter, bad) in [
        (true, None),
        (true, Some(json!([]))),
        (false, Some(json!([5]))),
        (true, Some(json!([4]))),
        (true, Some(json!([5, 5]))),
        (true, Some(json!([25]))),
        (true, Some(json!(["5"]))),
    ] {
        assert!(
            hours(filter, bad.clone()).is_err(),
            "accepted {filter} {bad:?}"
        );
    }
}
