use forex_bot::upscale::{parse_quotes, Client, Error};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};
const ID: &str = "00000000-0000-4000-8000-000000000001";
fn market(ticker: &str, base: &str) -> Value {
    json!({"id":ID,"config":{"ticker":ticker,"baseAsset":base,"quoteAsset":"USD"},"state":{"indexPrice":"159889999999","blockTimestamp":1000},"settings":{"isClosed":false,"isPaused":false,"isCloseOnly":false},"schedule":null})
}
fn schedule(in_pause: bool) -> Value {
    json!({"inPause":in_pause,"pausedAt":if in_pause { 1000 } else { 0 },
        "nextPauseAt":2000,"nextUnpauseAt":3000,
        "closestPauseAt":2000,"closestUnpauseAt":3000})
}

#[test]
fn schedule_is_preserved_including_zero_and_no_next_session() {
    for next in [0_i64, 2000, 9_007_199_254_740_991] {
        let mut m = market("EUR/USD", "EUR");
        m["schedule"] = schedule(false);
        for field in [
            "nextPauseAt",
            "nextUnpauseAt",
            "closestPauseAt",
            "closestUnpauseAt",
        ] {
            m["schedule"][field] = json!(next);
        }
        let q = parse_quotes(&json!([m.clone()]), &["EURUSD".into()], 1000, 1).unwrap();
        assert_eq!(
            serde_json::to_value(&q[0]).unwrap()["schedule"],
            m["schedule"]
        );
        assert!(q[0].to_tick().is_ok());
    }
    let q = parse_quotes(
        &json!([market("EUR/USD", "EUR")]),
        &["EURUSD".into()],
        1000,
        1,
    )
    .unwrap();
    assert!(q[0].to_tick().is_ok());
    assert_eq!(
        serde_json::to_value(&q[0]).unwrap().get("schedule"),
        Some(&Value::Null)
    );
}

#[test]
fn schedule_missing_or_malformed_is_not_a_round_the_clock_market() {
    let mut missing = market("EUR/USD", "EUR");
    missing.as_object_mut().unwrap().remove("schedule");
    assert!(parse_quotes(&json!([missing]), &["EURUSD".into()], 1000, 1).is_err());
    for value in [json!(false), json!([]), json!({}), json!({"inPause":false})] {
        let mut m = market("EUR/USD", "EUR");
        m["schedule"] = value;
        assert!(parse_quotes(&json!([m]), &["EURUSD".into()], 1000, 1).is_err());
    }
    for field in [
        "inPause",
        "pausedAt",
        "nextPauseAt",
        "nextUnpauseAt",
        "closestPauseAt",
        "closestUnpauseAt",
    ] {
        let mut m = market("EUR/USD", "EUR");
        m["schedule"] = schedule(false);
        m["schedule"].as_object_mut().unwrap().remove(field);
        assert!(
            parse_quotes(&json!([m]), &["EURUSD".into()], 1000, 1).is_err(),
            "missing {field}"
        );
        for bad in [Value::Null, json!("0"), json!(-1), json!(0.5)] {
            let mut m = market("EUR/USD", "EUR");
            m["schedule"] = schedule(false);
            m["schedule"][field] = bad;
            assert!(
                parse_quotes(&json!([m]), &["EURUSD".into()], 1000, 1).is_err(),
                "invalid {field}"
            );
        }
    }
}

#[test]
fn scheduled_pause_blocks_fresh_ticks() {
    let mut m = market("EUR/USD", "EUR");
    m["schedule"] = schedule(true);
    let q = parse_quotes(&json!([m]), &["EURUSD".into()], 1000, 1).unwrap();
    assert_eq!(q[0].to_tick(), Err(Error::Invalid("market is unavailable")));
}

#[test]
fn scheduled_pause_accepts_old_snapshot_only_as_status_then_checks_freshness() {
    let mut m = market("EUR/USD", "EUR");
    m["schedule"] = schedule(true);
    for _ in 0..2 {
        let q = parse_quotes(&json!([m.clone()]), &["EURUSD".into()], 100000, 1).unwrap();
        assert!(q[0].to_tick().is_err());
    }
    m["schedule"] = schedule(false);
    assert_eq!(
        parse_quotes(&json!([m.clone()]), &["EURUSD".into()], 100000, 1),
        Err(Error::Invalid("stale snapshot"))
    );
    m["state"]["blockTimestamp"] = json!(100000);
    let q = parse_quotes(&json!([m]), &["EURUSD".into()], 100000, 1).unwrap();
    assert!(q[0].to_tick().is_ok());
}
fn symbols() -> Vec<String> {
    vec!["EURUSD".into(), "GBPUSD".into(), "USDJPY".into()]
}
fn server(status: u16, body: Value, extra: &str) -> (String, thread::JoinHandle<String>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let body = body.to_string();
    let extra = extra.to_owned();
    l.set_nonblocking(true).unwrap();
    let h = thread::spawn(move || {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        let mut s = loop {
            if let Ok((s, _)) = l.accept() {
                break s;
            }
            if std::time::Instant::now() >= deadline {
                return String::new();
            }
            thread::sleep(std::time::Duration::from_millis(5));
        };
        s.set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut raw = vec![];
        let mut buf = [0; 1024];
        while !raw.windows(4).any(|w| w == b"\r\n\r\n") {
            let n = s.read(&mut buf).unwrap();
            if n == 0 {
                break;
            }
            raw.extend_from_slice(&buf[..n]);
        }
        write!(s,"HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n{extra}\r\n{body}",body.len()).unwrap();
        String::from_utf8(raw).unwrap()
    });
    (base, h)
}
#[test]
fn exact_prices_mapping_and_clock_boundaries() {
    let input = json!([
        market("EUR/USD", "EUR"),
        market("GBP/USD", "GBP"),
        market("USDJPY/USD", "USDJPY")
    ]);
    let quotes = parse_quotes(&input, &symbols(), 2000, 1).unwrap();
    assert_eq!(quotes.len(), 3);
    assert_eq!(quotes[2].symbol, "USDJPY");
    assert_eq!(quotes[2].index_price_fp9, 159889999999);
    assert_eq!(quotes[2].millis, 1000);
    assert_eq!(quotes[2].source, "upscale");
    assert_eq!(quotes[2].quote_asset, "USD");
    assert!(parse_quotes(&input, &symbols(), 2001, 1).is_err());
    assert!(parse_quotes(&input, &symbols(), 999, 1).is_err());
}
#[test]
fn closed_and_paused_are_explicit_even_when_stale() {
    for field in ["isClosed", "isPaused"] {
        let mut m = market("EUR/USD", "EUR");
        m["settings"][field] = json!(true);
        let q = parse_quotes(&json!([m]), &["EURUSD".into()], 100000, 1).unwrap();
        assert!(q[0].closed || q[0].paused);
    }
    let mut m = market("EUR/USD", "EUR");
    m["settings"]["isCloseOnly"] = json!(true);
    let q = parse_quotes(&json!([m.clone()]), &["EURUSD".into()], 1000, 1).unwrap();
    assert!(q[0].close_only);
    assert!(parse_quotes(&json!([m]), &["EURUSD".into()], 100000, 1).is_err());
}
#[test]
fn malformed_missing_duplicate_and_unknown_markets_fail() {
    let valid = market("EUR/USD", "EUR");
    for (path, value) in [
        ("/id", json!("bad")),
        ("/config/baseAsset", json!("GBP")),
        ("/config/quoteAsset", json!("JPY")),
        ("/state/indexPrice", json!("0")),
        ("/state/indexPrice", json!("1.2")),
        ("/state/indexPrice", json!("9223372036854775808")),
        ("/state/indexPrice", json!(1)),
        ("/state/blockTimestamp", json!(-1)),
        ("/settings/isPaused", Value::Null),
    ] {
        let mut m = valid.clone();
        *m.pointer_mut(path).unwrap() = value;
        assert!(
            parse_quotes(&json!([m]), &["EURUSD".into()], 1000, 1).is_err(),
            "{path}"
        );
    }
    for input in [json!([]), json!([valid.clone(), valid]), json!({})] {
        assert!(parse_quotes(&input, &["EURUSD".into()], 1000, 1).is_err());
    }
    assert!(parse_quotes(
        &json!([market("AUD/USD", "AUD")]),
        &["AUDUSD".into()],
        1000,
        1
    )
    .is_err());
}
#[test]
fn accounts_use_get_bearer_and_filter_real_inactive_and_disabled() {
    let body = json!([{"accountId":ID,"type":"demo","status":"active","apiTrading":true},
        {"type":"real","status":"active","apiTrading":true},
        {"type":"demo","status":"failed","apiTrading":true},
        {"type":"demo","status":"active","apiTrading":false}]);
    let (url, h) = server(200, body, "");
    let result = Client::new(&url, "fake-upscale-key")
        .unwrap()
        .demo_accounts();
    let req = h.join().unwrap();
    assert_eq!(result.unwrap(), vec![ID]);
    assert!(req.starts_with("GET /accounts/with-risk-status "));
    assert!(req
        .to_lowercase()
        .contains("authorization: bearer fake-upscale-key"));
}
#[test]
fn one_market_request_returns_all_symbols_for_account() {
    let mut eur = market("EUR/USD", "EUR");
    eur["schedule"] = schedule(true);
    let mut gbp = market("GBP/USD", "GBP");
    gbp["schedule"] = schedule(false);
    let (url, h) = server(200, json!([eur, gbp, market("USDJPY/USD", "USDJPY")]), "");
    let result = Client::new(&url, "fake-upscale-key")
        .unwrap()
        .quotes(ID, &symbols(), || 1000, 1);
    let req = h.join().unwrap();
    let quotes = result.unwrap();
    assert_eq!(quotes.len(), 3);
    assert!(quotes[0].to_tick().is_err());
    assert!(quotes[1].to_tick().is_ok());
    assert!(quotes[2].to_tick().is_ok());
    assert!(req.starts_with(&format!("GET /v2/markets?accountId={ID} ")));
}
#[test]
fn http_failures_keep_status_retry_after_without_body_or_key() {
    for status in [403, 429, 302] {
        let (url, h) = server(
            status,
            json!({"message":"fake-upscale-key"}),
            "Retry-After: 17\r\nLocation: http://127.0.0.1:1/\r\n",
        );
        let result = Client::new(&url, "fake-upscale-key")
            .unwrap()
            .demo_accounts();
        h.join().unwrap();
        assert_eq!(
            result.unwrap_err(),
            Error::Http {
                status,
                code: None,
                retry_after_seconds: Some(17)
            }
        );
    }
}

fn index_rules() -> Value {
    let mut v: Value =
        serde_json::from_str(include_str!("../../strategy/fvg-bot-rules.json")).unwrap();
    v["candles"]["price_basis"] = json!("index");
    v
}
#[test]
fn index_strategy_and_integer_rounding_are_explicit() {
    let cfg = serde_json::from_value(index_rules()).unwrap();
    forex_bot::validate(&cfg).unwrap();
    for (raw, expected) in [
        ("159889999999", 159890000),
        ("1106250499", 1106250),
        ("1106250500", 1106251),
    ] {
        let mut m = market("EUR/USD", "EUR");
        m["state"]["indexPrice"] = json!(raw);
        let q = parse_quotes(&json!([m]), &["EURUSD".into()], 1000, 1)
            .unwrap()
            .remove(0);
        let t = q.to_tick().unwrap();
        assert_eq!(t.price, expected);
        assert_eq!(t.source, "upscale:index");
    }
}
#[test]
fn index_runtime_restores_snapshot_and_rejects_other_sources() {
    use forex_bot::{engine::Engine, feed::Tick, runtime::Runtime};
    let p = std::env::temp_dir().join(format!("upscale-index-{}.sqlite", std::process::id()));
    let mut r = Runtime::open(&p, index_rules(), Engine::default()).unwrap();
    let t = Tick {
        symbol: "EURUSD".into(),
        millis: 2000,
        price: 100,

        source: "upscale:index".into(),
    };
    r.tick(t.clone(), false).unwrap();
    drop(r);
    let mut r = Runtime::open(&p, index_rules(), Engine::default()).unwrap();
    let pending = r.pending().unwrap().len();
    r.tick(t, false).unwrap();
    assert_eq!(r.pending().unwrap().len(), pending);
    let wrong = Tick {
        symbol: "EURUSD".into(),
        millis: 3000,
        price: 100,

        source: "other-source".into(),
    };
    assert!(r.tick(wrong, false).is_err());
}
