use forex_bot::http::{Api, Failure};
use serde_json::json;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};
fn server(status: u16, body: &str) -> (String, thread::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let body = body.to_string();
    let handle = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let mut raw = Vec::new();
        let mut buf = [0; 4096];
        loop {
            let n = stream.read(&mut buf).unwrap();
            raw.extend_from_slice(&buf[..n]);
            if let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                let headers = String::from_utf8_lossy(&raw[..end]).to_lowercase();
                let size: usize = headers
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length: "))
                    .unwrap_or("0")
                    .parse()
                    .unwrap();
                if raw.len() >= end + 4 + size {
                    break;
                }
            }
        }
        write!(
            stream,
            "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        String::from_utf8(raw).unwrap()
    });
    (url, handle)
}
#[test]
fn telegram_posts_offset_and_returns_updates() {
    let (url, h) = server(200, r#"{"ok":true,"result":[{"update_id":7}]}"#);
    let api = Api::new().unwrap();
    let updates = api
        .telegram(
            &url,
            "fake-test-token",
            "getUpdates",
            &json!({"offset":7,"timeout":20,"allowed_updates":["message"]}),
        )
        .unwrap();
    assert_eq!(updates[0]["update_id"], 7);
    let request = h.join().unwrap();
    assert!(request.starts_with("POST /botfake-test-token/getUpdates "));
    assert!(request.contains("\"offset\":7"));
}
#[test]
fn telegram_api_errors_are_classified_even_at_http_200() {
    for (code, expected) in [
        (401, Failure::Access),
        (429, Failure::Retry),
        (400, Failure::Invalid),
    ] {
        let (url, h) = server(
            200,
            &json!({"ok":false,"error_code":code,"description":"sensitive-test-value"}).to_string(),
        );
        assert_eq!(
            Api::new()
                .unwrap()
                .telegram(&url, "fake-test-token", "sendMessage", &json!({}))
                .unwrap_err(),
            expected
        );
        h.join().unwrap();
    }
}
#[test]
fn http_errors_and_redirects_do_not_expose_credentials() {
    for (status, expected) in [
        (503, Failure::Retry),
        (403, Failure::Access),
        (302, Failure::Invalid),
    ] {
        let (url, h) = server(status, "sensitive-test-value");
        let error = Api::new()
            .unwrap()
            .telegram(&url, "fake-test-token", "getUpdates", &json!({}))
            .unwrap_err();
        assert_eq!(error, expected);
        assert!(!format!("{error:?} {error}").contains("fake-test-token"));
        h.join().unwrap();
    }
}
#[test]
fn openai_uses_bearer_and_parses_json_without_echoing_invalid_response() {
    let (url, h) = server(200, r#"{"status":"completed"}"#);
    let result = Api::new()
        .unwrap()
        .responses(&url, "fake-test-key", &json!({"store":false}))
        .unwrap();
    assert_eq!(result["status"], "completed");
    let request = h.join().unwrap().to_lowercase();
    assert!(request.starts_with("post /v1/responses "));
    assert!(request.contains("authorization: bearer fake-test-key"));
    let (url, h) = server(200, "secret-looking-invalid-json");
    assert_eq!(
        Api::new()
            .unwrap()
            .responses(&url, "fake-test-key", &json!({}))
            .unwrap_err(),
        Failure::Invalid
    );
    h.join().unwrap();
}
#[test]
fn unsafe_download_paths_are_rejected_before_network() {
    for path in [
        "../token",
        "https://elsewhere/file",
        "/absolute",
        "photo?token=x",
        "a/../b",
    ] {
        assert_eq!(
            Api::new()
                .unwrap()
                .file("http://127.0.0.1:1", "fake-test-token", path)
                .unwrap_err(),
            Failure::Invalid
        );
    }
}
#[test]
fn telegram_can_read_configured_group_without_sending_messages() {
    let (url, h) = server(
        200,
        r#"{"ok":true,"result":{"id":-100123,"type":"supergroup"}}"#,
    );
    let result = Api::new()
        .unwrap()
        .telegram(
            &url,
            "fake-test-token",
            "getChat",
            &json!({"chat_id":-100123}),
        )
        .unwrap();
    assert_eq!(result["type"], "supergroup");
    let request = h.join().unwrap();
    assert!(request.starts_with("POST /botfake-test-token/getChat "));
}
