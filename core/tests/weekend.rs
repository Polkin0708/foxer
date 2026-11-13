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
#[test]
fn weekend_closure_keeps_weekly_bar_complete() {
    let mut a = Aggregator::default();
    week(&mut a, "2026-01-04T22:00:00Z", "2026-01-09T22:00:00Z");
    let closed = a
        .minute(
            "EURUSD",
            "s",
            &m(utc("2026-01-11T22:00:00Z").unwrap(), 2880),
        )
        .unwrap();
    let tfs: Vec<_> = closed.iter().map(|(tf, _)| *tf).collect();
    assert_eq!(tfs, [Timeframe::W, Timeframe::D, Timeframe::H4]);
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
    assert!(closed.iter().any(|(tf, _)| *tf == Timeframe::W));
}
#[test]
fn gap_inside_the_week_still_breaks_the_weekly_bar() {
    let mut a = Aggregator::default();
    week(&mut a, "2026-01-04T22:00:00Z", "2026-01-07T10:00:00Z");
    week(&mut a, "2026-01-07T12:00:00Z", "2026-01-09T22:00:00Z");
    let closed = a
        .minute(
            "EURUSD",
            "s",
            &m(utc("2026-01-11T22:00:00Z").unwrap(), 2880),
        )
        .unwrap();
    assert!(closed.iter().all(|(tf, _)| *tf != Timeframe::W));
}
