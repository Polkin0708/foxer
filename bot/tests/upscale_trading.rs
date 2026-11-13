use forex_bot::upscale::{
    Client, CloseMarketOrder, Direction, Error, OpenMarketOrder, OrderStatus,
};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
    time::{Duration, Instant},
};

const ACCOUNT: &str = "00000000-0000-4000-8000-000000000001";
const MARKET: &str = "00000000-0000-4000-8000-000000000002";
const POSITION: &str = "00000000-0000-4000-8000-000000000003";
const ORDER: &str = "00000000-0000-4000-8000-000000000004";
const KEY: &str = "fixture-upscale-key";

struct Request {
    headers: String,
    body: Value,
}

// Respond to every observed request, including unexpected retries, and report
// them to the test. No live API or real credentials are used.
fn server(replies: Vec<(u16, String)>) -> (Client, thread::JoinHandle<Vec<Request>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let client = Client::new(&format!("http://{}", listener.local_addr().unwrap()), KEY).unwrap();
    let handle = thread::spawn(move || {
        let mut requests = Vec::new();
        loop {
            let wait = if requests.len() >= replies.len() {
                200
            } else {
                3000
            };
            let deadline = Instant::now() + Duration::from_millis(wait);
            let mut stream = loop {
                if let Ok((stream, _)) = listener.accept() {
                    break stream;
                }
                if Instant::now() >= deadline {
                    return requests;
                }
                thread::sleep(Duration::from_millis(5));
            };
            stream
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut raw = Vec::new();
            let mut buf = [0; 4096];
            let (headers, body) = loop {
                let n = stream.read(&mut buf).unwrap();
                assert!(n > 0);
                raw.extend_from_slice(&buf[..n]);
                if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8(raw[..end].to_vec()).unwrap();
                    let len = headers
                        .to_lowercase()
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("content-length: ")
                                .map(|s| s.parse::<usize>().unwrap())
                        })
                        .unwrap_or(0);
                    if raw.len() >= end + 4 + len {
                        let body = if len == 0 {
                            Value::Null
                        } else {
                            serde_json::from_slice(&raw[end + 4..end + 4 + len]).unwrap()
                        };
                        break (headers, body);
                    }
                }
            };
            let (status, response_body) = &replies[requests.len().min(replies.len() - 1)];
            requests.push(Request { headers, body });
            if *status != 0 {
                // The client may close early when Content-Length exceeds its limit.
                let _ = write!(stream, "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\nRetry-After: 17\r\nLocation: http://127.0.0.1:1/\r\n\r\n{response_body}", response_body.len());
            }
            assert!(
                requests.len() <= replies.len() + 2,
                "unexpected repeated requests"
            );
        }
    });
    (client, handle)
}

fn reply(status: &str) -> String {
    json!({"id":ORDER,"status":status,"errorCode":null}).to_string()
}
fn opening(direction: Direction) -> OpenMarketOrder<'static> {
    OpenMarketOrder {
        account_id: ACCOUNT,
        market_id: MARKET,
        direction,
        amount: "9223372036854775808123",
        leverage: "5000000000",
    }
}
fn closing(direction: Direction, amount: &str) -> CloseMarketOrder<'_> {
    CloseMarketOrder {
        account_id: ACCOUNT,
        market_id: MARKET,
        position_id: POSITION,
        direction,
        amount,
    }
}
fn assert_post(request: &Request, key: &str) {
    assert!(request.headers.starts_with("POST /orders HTTP/1.1\r\n"));
    let headers = request.headers.to_lowercase();
    assert!(headers.contains(&format!("authorization: bearer {KEY}")));
    assert!(headers.contains("content-type: application/json"));
    assert!(headers.contains("accept: application/json"));
    assert!(headers
        .lines()
        .any(|l| l == format!("x-idempotency-key: {key}")));
}

#[test]
fn market_open_preserves_fp9_and_caller_key_on_explicit_repeat() {
    let (client, server) = server(vec![(200, reply("executed")), (200, reply("executed"))]);
    let first = client.open_market(&opening(Direction::Long), "same-open-key");
    let second = client.open_market(&opening(Direction::Long), "same-open-key");
    let requests = server.join().unwrap();
    assert_eq!(first.unwrap(), second.unwrap());
    assert_eq!(requests.len(), 2);
    for r in &requests {
        assert_post(r, "same-open-key");
        assert_eq!(
            r.body,
            json!({"accountId":ACCOUNT,"marketId":MARKET,"type":"market",
            "direction":"long","amount":"9223372036854775808123","leverage":"5000000000"})
        );
    }
}

#[test]
fn read_positions_then_close_full_or_partial_base_size_without_reversing_direction() {
    let size = "9007199254740993";
    let positions = json!([
        {"idx":POSITION,"market":MARKET,"direction":"short","size":size},
        {"idx":ORDER,"market":MARKET,"direction":"long","size":"1000000000"}
    ]);
    let (client, server) = server(vec![
        (200, positions.to_string()),
        (200, reply("executed")),
        (200, reply("executed")),
    ]);
    let positions = client.active_positions(ACCOUNT).unwrap();
    assert_eq!(positions.len(), 2);
    let p = &positions[0];
    assert_eq!(p.id, POSITION);
    assert_eq!(p.market_id, MARKET);
    assert_eq!(p.direction, Direction::Short);
    assert_eq!(p.size, size);
    assert_eq!(
        client
            .close_market(&closing(p.direction, &p.size), "full-close")
            .unwrap()
            .status,
        OrderStatus::Executed
    );
    client
        .close_market(&closing(Direction::Long, "123456789"), "partial-close")
        .unwrap();
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 3);
    assert!(requests[0]
        .headers
        .starts_with(&format!("GET /positions/{ACCOUNT}/active HTTP/1.1\r\n")));
    assert!(requests[0]
        .headers
        .to_lowercase()
        .contains(&format!("authorization: bearer {KEY}")));
    assert_eq!(requests[0].body, Value::Null);
    for (r, direction, amount, key) in [
        (&requests[1], "short", size, "full-close"),
        (&requests[2], "long", "123456789", "partial-close"),
    ] {
        assert_post(r, key);
        assert_eq!(
            r.body,
            json!({"accountId":ACCOUNT,"marketId":MARKET,"positionId":POSITION,
            "type":"take","triggerPrice":"0","direction":direction,"amount":amount})
        );
    }
}

#[test]
fn order_status_and_execution_error_are_not_replaced_by_success() {
    let cases = [
        ("active", OrderStatus::Active),
        ("executed", OrderStatus::Executed),
        ("canceled", OrderStatus::Canceled),
        ("canceled_by_update", OrderStatus::CanceledByUpdate),
        ("canceled_by_position", OrderStatus::CanceledByPosition),
        ("canceled_by_error", OrderStatus::CanceledByError),
    ];
    let replies = cases.iter().map(|(status, _)| (200, json!({"id":ORDER,"status":status,
        "errorCode":if *status == "canceled_by_error" { Some("insufficient_reserve_at_execution") } else { None }}).to_string())).collect();
    let (client, server) = server(replies);
    for (status, expected) in cases {
        let order = client
            .open_market(&opening(Direction::Short), status)
            .unwrap();
        assert_eq!(order.id, ORDER);
        assert_eq!(order.status, expected);
        assert_eq!(
            order.error_code.as_deref(),
            if status == "canceled_by_error" {
                Some("insufficient_reserve_at_execution")
            } else {
                None
            }
        );
    }
    let requests = server.join().unwrap();
    assert_eq!(requests.len(), 6);
    assert!(requests.iter().all(|r| r.body["direction"] == "short"));
}

#[test]
fn http_errors_keep_codes_and_retry_after_without_retrying_or_leaking_messages() {
    let cases = [
        (400, "insufficient_balance"),
        (403, "market_paused"),
        (409, "idempotency_key_in_flight"),
        (429, "api_key_rate_limit_exceeded"),
        (500, "internal_error"),
        (302, "redirect"),
    ];
    let (client, server) = server(
        cases
            .iter()
            .map(|(s, code)| (*s, json!({"error":code,"message":KEY}).to_string()))
            .collect(),
    );
    for (status, code) in cases {
        let error = client
            .close_market(&closing(Direction::Long, "1000000000"), "close-key")
            .unwrap_err();
        assert_eq!(
            error,
            Error::Http {
                status,
                retry_after_seconds: Some(17),
                code: Some(code.into())
            }
        );
        assert!(!format!("{error:?} {error}").contains(KEY));
        assert!(!format!("{error:?}").contains("message"));
    }
    assert_eq!(server.join().unwrap().len(), 6);
}

#[test]
fn invalid_error_bodies_keep_http_status_without_echoing_credentials() {
    let bodies = [
        KEY.to_string(),
        json!({"error":KEY,"message":KEY}).to_string(),
        json!({"error":"usk_fixture_key","message":KEY}).to_string(),
        "x".repeat(2 * 1024 * 1024 + 1),
    ];
    let (client, server) = server(bodies.iter().map(|s| (403, s.clone())).collect());
    for _ in bodies {
        let error = client
            .open_market(&opening(Direction::Long), "open-key")
            .unwrap_err();
        assert_eq!(
            error,
            Error::Http {
                status: 403,
                retry_after_seconds: Some(17),
                code: None
            }
        );
        assert!(!format!("{error:?} {error}").contains(KEY));
    }
    assert_eq!(server.join().unwrap().len(), 4);
}

#[test]
fn transport_failure_does_not_repeat_post() {
    let (client, server) = server(vec![(0, String::new())]);
    assert_eq!(
        client.open_market(&opening(Direction::Long), "uncertain-open"),
        Err(Error::Transport)
    );
    assert_eq!(server.join().unwrap().len(), 1);
}

#[test]
fn malformed_success_responses_are_rejected_and_empty_positions_are_valid() {
    let replies = vec![
        (200, "[]".into()),
        (200, "{}".into()),
        (
            200,
            json!([{"idx":null,"market":MARKET,"direction":"long","size":"1"}]).to_string(),
        ),
        (
            200,
            json!([{"idx":POSITION,"market":MARKET,"direction":"long","size":1}]).to_string(),
        ),
        (200, "not json".into()),
        (200, json!({"id":ORDER,"status":"unknown"}).to_string()),
        (200, json!({"id":"bad","status":"executed"}).to_string()),
        (200, "x".repeat(2 * 1024 * 1024 + 1)),
    ];
    let (client, server) = server(replies);
    assert!(client.active_positions(ACCOUNT).unwrap().is_empty());
    for _ in 0..3 {
        assert!(matches!(
            client.active_positions(ACCOUNT),
            Err(Error::Invalid(_))
        ));
    }
    for _ in 0..4 {
        assert!(matches!(
            client.open_market(&opening(Direction::Long), "open-key"),
            Err(Error::Invalid(_))
        ));
    }
    assert_eq!(server.join().unwrap().len(), 8);
}

#[test]
fn invalid_ids_fp9_and_idempotency_keys_fail_before_network() {
    let (client, server) = server(vec![(200, reply("executed"))]);
    for bad in ["", "0", "-1", "1.2", "1e9", "01"] {
        let mut order = opening(Direction::Long);
        order.amount = bad;
        assert_eq!(
            client.open_market(&order, "key"),
            Err(Error::Invalid("invalid fp9 amount"))
        );
        order = opening(Direction::Long);
        order.leverage = bad;
        assert_eq!(
            client.open_market(&order, "key"),
            Err(Error::Invalid("invalid fp9 leverage"))
        );
        assert_eq!(
            client.close_market(&closing(Direction::Long, bad), "key"),
            Err(Error::Invalid("invalid fp9 amount"))
        );
    }
    for key in ["", " ", "key\r\nx: y"] {
        assert_eq!(
            client.open_market(&opening(Direction::Long), key),
            Err(Error::Invalid("invalid idempotency key"))
        );
        assert_eq!(
            client.close_market(&closing(Direction::Long, "1"), key),
            Err(Error::Invalid("invalid idempotency key"))
        );
    }
    assert_eq!(
        client.active_positions("bad/id"),
        Err(Error::Invalid("invalid account ID"))
    );
    let mut order = opening(Direction::Long);
    order.account_id = "bad";
    assert_eq!(
        client.open_market(&order, "key"),
        Err(Error::Invalid("invalid account ID"))
    );
    order = opening(Direction::Long);
    order.market_id = "bad";
    assert_eq!(
        client.open_market(&order, "key"),
        Err(Error::Invalid("invalid market ID"))
    );
    let mut order = closing(Direction::Long, "1");
    order.position_id = "bad";
    assert_eq!(
        client.close_market(&order, "key"),
        Err(Error::Invalid("invalid position ID"))
    );
    assert!(server.join().unwrap().is_empty());
}
