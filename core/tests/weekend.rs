use forex_core::{
    engine::Timeframe,
    feed::{Aggregator, Tick},
    prices::{utc, Minute},
};
fn m(t: i64, gap: u64) -> Minute {
    Minute {
        timestamp: t,
        price: [100, 101, 99, 100],
        quotes: 1,
        gap_before_minutes: gap,
    }
}
fn week(a: &mut Aggregator, from: &str, to: &str) {
    let (mut t, end) = (utc(from).unwrap(), utc(to).unwrap());
    while t < end {
        a.minute("EURUSD", "s", &m(t, 0)).unwrap();
        t += 60;
    }
}
fn tfs(closed: &[(Timeframe, forex_core::trading::Candle)]) -> Vec<Timeframe> {
    closed.iter().map(|(tf, _)| *tf).collect()
}
#[test]
fn weekend_closure_keeps_weekly_and_daily_bars_complete() {
    let mut a = Aggregator::default();
    // Broker closes 16:55 and reopens 17:02 New York (winter: 21:55Z / 22:02Z).
    week(&mut a, "2026-01-04T22:00:00Z", "2026-01-09T21:55:00Z");
    let closed = a
        .minute(
            "EURUSD",
            "s",
            &m(utc("2026-01-11T22:02:00Z").unwrap(), 2886),
        )
        .unwrap();
    assert_eq!(tfs(&closed), [Timeframe::W, Timeframe::D, Timeframe::H4]);
    // Monday's daily bar continues completeness from Friday's.
    week(&mut a, "2026-01-11T22:03:00Z", "2026-01-12T22:00:00Z");
    let closed = a
        .minute("EURUSD", "s", &m(utc("2026-01-12T22:00:00Z").unwrap(), 0))
        .unwrap();
    assert!(tfs(&closed).contains(&Timeframe::D));
    // A week that opened late still closes complete the following Sunday.
    week(&mut a, "2026-01-12T22:01:00Z", "2026-01-16T21:55:00Z");
    let closed = a
        .minute(
            "EURUSD",
            "s",
            &m(utc("2026-01-18T22:02:00Z").unwrap(), 2886),
        )
        .unwrap();
    assert!(tfs(&closed).contains(&Timeframe::W));
}
#[test]
fn weekend_closure_keeps_weekly_bar_complete_for_ticks_and_in_summer() {
    let mut a = Aggregator::default();
    // Summer: 17:00 New York = 21:00Z
    week(&mut a, "2026-07-05T21:00:00Z", "2026-07-10T21:00:00Z");
    let closed = a
        .push(Tick {
            symbol: "EURUSD".into(),
            source: "s".into(),
            millis: utc("2026-07-12T21:00:00Z").unwrap() * 1000,
            price: 100,
        })
        .unwrap();
    assert!(tfs(&closed).contains(&Timeframe::W));
}
#[test]
fn tickless_minutes_do_not_break_prepared_bars_but_declared_gaps_do() {
    let mut a = Aggregator::default();
    week(&mut a, "2026-01-04T22:00:00Z", "2026-01-07T10:00:00Z");
    // three minutes without ticks, declared as no gap by the adapter
    week(&mut a, "2026-01-07T10:03:00Z", "2026-01-09T22:00:00Z");
    let closed = a
        .minute(
            "EURUSD",
            "s",
            &m(utc("2026-01-11T22:00:00Z").unwrap(), 2880),
        )
        .unwrap();
    assert!(tfs(&closed).contains(&Timeframe::W));
    let mut a = Aggregator::default();
    week(&mut a, "2026-01-04T22:00:00Z", "2026-01-07T10:00:00Z");
    a.minute("EURUSD", "s", &m(utc("2026-01-07T10:03:00Z").unwrap(), 2))
        .unwrap();
    week(&mut a, "2026-01-07T10:04:00Z", "2026-01-09T22:00:00Z");
    let closed = a
        .minute(
            "EURUSD",
            "s",
            &m(utc("2026-01-11T22:00:00Z").unwrap(), 2880),
        )
        .unwrap();
    assert!(!tfs(&closed).contains(&Timeframe::W));
}
#[test]
fn declared_gap_inside_the_week_still_breaks_the_weekly_bar() {
    let mut a = Aggregator::default();
    week(&mut a, "2026-01-04T22:00:00Z", "2026-01-07T10:00:00Z");
    a.minute("EURUSD", "s", &m(utc("2026-01-07T12:00:00Z").unwrap(), 119))
        .unwrap();
    week(&mut a, "2026-01-07T12:01:00Z", "2026-01-09T22:00:00Z");
    let closed = a
        .minute(
            "EURUSD",
            "s",
            &m(utc("2026-01-11T22:00:00Z").unwrap(), 2880),
        )
        .unwrap();
    assert!(!tfs(&closed).contains(&Timeframe::W));
}
