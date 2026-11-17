//! Swing rules in the live runtime (requirements: Правила «свинг» в работе бота).
use forex_bot::{engine::Engine, feed::Tick, runtime::Runtime};
use forex_core::swing::{Kind, Swing, SwingConfig, Tf};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
fn path() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "swing-runtime-{}-{}.sqlite",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ))
}
fn rules() -> Value {
    serde_json::from_str(include_str!("../../strategy/fvg-bot-rules.json")).unwrap()
}
fn config() -> Value {
    json!({"cost": 0.0001})
}
/// Rules with one long trend plan confirmed and entered on an H4 gap.
fn planned() -> Swing {
    let mut s = Swing::new(serde_json::from_value::<SwingConfig>(config()).unwrap());
    s.add_plan(Kind::Trend, 1, (1.0, 1.05), 1.2, Tf::H4);
    s
}
fn tick(at: &str, price: f64) -> Tick {
    Tick {
        symbol: "EURUSD".into(),
        millis: chrono::DateTime::parse_from_rfc3339(at)
            .unwrap()
            .timestamp_millis(),
        price: (price * 1_000_000.0).round() as i64,
        source: "upscale:index".into(),
    }
}
/// Three H4 bars on 2026-01-05 whose third leaves a bullish gap; the 22:00Z tick closes it.
fn gap_ticks(r: &mut Runtime) {
    for (at, p) in [
        ("2026-01-05T10:00:00Z", 1.10000),
        ("2026-01-05T10:30:00Z", 1.10100),
        ("2026-01-05T11:00:00Z", 1.09900),
        ("2026-01-05T13:59:00Z", 1.10050),
        ("2026-01-05T14:00:00Z", 1.10050),
        ("2026-01-05T15:00:00Z", 1.10300),
        ("2026-01-05T17:59:00Z", 1.10280),
        ("2026-01-05T18:00:00Z", 1.10280),
        ("2026-01-05T19:00:00Z", 1.10300),
        ("2026-01-05T21:59:00Z", 1.10250),
        ("2026-01-05T22:00:00Z", 1.10250),
    ] {
        r.tick(tick(at, p), true).unwrap();
    }
}
fn texts(r: &Runtime) -> Vec<String> {
    r.pending()
        .unwrap()
        .into_iter()
        .map(|o| o.payload["text"].as_str().unwrap_or_default().to_owned())
        .collect()
}
fn count(r: &Runtime, needle: &str) -> usize {
    texts(r).iter().filter(|t| t.contains(needle)).count()
}

#[test]
fn swing_trade_is_published_with_its_levels() {
    let p = path();
    let mut r = Runtime::open(&p, rules(), Engine::default()).unwrap();
    r.swing("EURUSD", config(), || Ok(planned())).unwrap();
    gap_ticks(&mut r);
    assert_eq!(count(&r, "EUR/USD · покупка · тренд"), 1);
    assert_eq!(count(&r, "Сделка открыта"), 1);
    let open = texts(&r)
        .into_iter()
        .find(|t| t.contains("Сделка открыта"))
        .unwrap();
    assert!(open.contains("Вход 1.10260"), "{open}");
    std::fs::remove_file(p).ok();
}

#[test]
fn state_survives_a_restart_without_repeating_messages() {
    let p = path();
    {
        let mut r = Runtime::open(&p, rules(), Engine::default()).unwrap();
        r.swing("EURUSD", config(), || Ok(planned())).unwrap();
        gap_ticks(&mut r);
    }
    let mut r = Runtime::open(&p, rules(), Engine::default()).unwrap();
    r.swing("EURUSD", config(), || Err("must not warm up again".into()))
        .unwrap();
    assert_eq!(count(&r, "Сделка открыта"), 1);
    r.tick(tick("2026-01-05T22:30:00Z", 1.09000), true).unwrap();
    assert_eq!(count(&r, "Стоп"), 1);
    std::fs::remove_file(p).ok();
}

#[test]
fn open_trades_from_the_warm_up_are_announced_once() {
    let p = path();
    let warm = {
        let mut r = Runtime::open(&p, rules(), Engine::default()).unwrap();
        r.swing("EURUSD", config(), || Ok(planned())).unwrap();
        gap_ticks(&mut r);
        r.swing_rules("EURUSD").unwrap().clone()
    };
    let p2 = path();
    let mut r = Runtime::open(&p2, rules(), Engine::default()).unwrap();
    r.swing("EURUSD", config(), || Ok(warm.clone())).unwrap();
    r.swing("EURUSD", config(), || Ok(warm.clone())).unwrap();
    assert_eq!(count(&r, "до запуска"), 1);
    for f in [p, p2] {
        std::fs::remove_file(f).ok();
    }
}

#[test]
fn changed_parameters_warm_up_again_and_index_exit_is_rejected() {
    let p = path();
    let mut r = Runtime::open(&p, rules(), Engine::default()).unwrap();
    r.swing("EURUSD", config(), || Ok(planned())).unwrap();
    let mut warmed = false;
    r.swing("EURUSD", json!({"cost": 0.0002}), || {
        warmed = true;
        Ok(planned())
    })
    .unwrap();
    assert!(warmed);
    assert!(r
        .swing(
            "EURUSD",
            json!({"reference_exit": {"sign": -1, "recent_events": 50}}),
            || Ok(planned())
        )
        .is_err());
    assert!(r.swing("XAUUSD", config(), || Ok(planned())).is_err());
    std::fs::remove_file(p).ok();
}
#[test]
fn warm_up_replays_stored_minutes_through_the_bot_aggregator() {
    let p = path();
    drop(Runtime::open(&p, rules(), Engine::default()).unwrap());
    let db = rusqlite::Connection::open(&p).unwrap();
    let start = forex_core::prices::utc("2026-01-05T00:00:00Z").unwrap();
    for i in 0..(5 * 24 * 60) {
        let m = forex_core::prices::Minute {
            timestamp: start + i * 60,
            price: [1_100_000; 4],
            quotes: 1,
            gap_before_minutes: 0,
        };
        db.execute(
            "INSERT INTO minutes VALUES('EURUSD','mt5',?1,?2)",
            rusqlite::params![m.timestamp, serde_json::to_string(&m).unwrap()],
        )
        .unwrap();
    }
    let swing = forex_bot::service::swing_warmup(&p, "EURUSD", &config()).unwrap();
    assert!(swing.frame(Tf::H4).len() >= 29);
    assert!(swing.frame(Tf::D).len() >= 4);
    assert!(forex_bot::service::swing_warmup(&p, "GBPUSD", &config()).is_err());
    db.execute(
        "INSERT INTO minutes VALUES('EURUSD','other',?1,'{}')",
        rusqlite::params![start],
    )
    .unwrap();
    assert!(forex_bot::service::swing_warmup(&p, "EURUSD", &config()).is_err());
}
