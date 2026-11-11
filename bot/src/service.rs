use crate::{
    engine::{Engine, Timeframe},
    feed::{self, Tick},
    http::{Api, Failure},
    protocols::{self, Analysis, Comment, Image},
    runtime::Runtime,
    settings::Settings,
    trading::Candle,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::Path,
    sync::mpsc,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const TELEGRAM: &str = "https://api.telegram.org";
const OPENAI: &str = "https://api.openai.com";
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}
fn rules(path: &Path) -> Result<Value, String> {
    forex_core::load_document(path)
}
pub fn check(path: &Path) -> Result<String, String> {
    let c = Settings::load(path)?;
    crate::load(&c.strategy)?;
    Ok("Service settings OK; no network requests performed.".into())
}
pub fn smoke(db: &Path, strategy: &Path) -> Result<String, String> {
    if db.exists() {
        return Err("smoke-test requires a new database path".into());
    }
    let rules = rules(strategy)?;
    let mut r = Runtime::open(db, rules.clone(), Engine::default())?;
    r.link(9, "smoke-signal")?;
    let update = json!({"update_id":1,"message":{"message_id":10,"chat":{"id":-1},"from":{"id":11},"text":"smoke","reply_to_message":{"message_id":9}}});
    r.update(&update, -1)?;
    drop(r);
    let mut r = Runtime::open(db, rules, Engine::default())?;
    r.update(&update, -1)?;
    if r.offset() != 2 || r.next_comment().map(|c| c.update_id) != Some(1) {
        return Err("smoke restore failed".into());
    }
    r.analysis(
        1,
        &Analysis {
            action: "no_change".into(),
            explanation: "Smoke test".into(),
            changes: vec![],
        },
        &json!({}),
    )?;
    let out = r.pending()?;
    if out.len() != 1 {
        return Err("smoke outbox failed".into());
    }
    r.delivered(&out[0].id, 11)?;
    if !r.pending()?.is_empty() {
        return Err("smoke delivery failed".into());
    }
    Ok("Smoke test OK: persistence, restart, replay, FIFO and delivery; offline.".into())
}
pub fn probe(path: &Path) -> Result<String, String> {
    let c = Settings::load(path)?;
    let api = Api::new().map_err(|e| e.to_string())?;
    let me = api
        .telegram(TELEGRAM, &c.telegram_token, "getMe", &json!({}))
        .map_err(|e| format!("Telegram: {e}"))?;
    if me["is_bot"] != true {
        return Err("Telegram identity is not a bot".into());
    }
    let chat = api
        .telegram(
            TELEGRAM,
            &c.telegram_token,
            "getChat",
            &json!({"chat_id":c.chat}),
        )
        .map_err(|e| format!("Telegram group: {e}"))?;
    if !matches!(chat["type"].as_str(), Some("group" | "supergroup")) {
        return Err("configured Telegram chat is not a group".into());
    }
    let cfg = crate::load(&c.strategy)?;
    for item in cfg.instruments {
        let market = api
            .market(&item.symbol)
            .map_err(|e| format!("Biquote {}: {e}", item.symbol))?;
        let tick = feed::parse_tick(&market)?;
        if tick.symbol != item.symbol {
            return Err("market symbol mismatch".into());
        }
    }
    Ok("Telegram access and all configured Biquote snapshots OK; OpenAI not called.".into())
}
fn initial_engine(database: &Path, strategy: &Path, symbol: &str) -> Result<Engine, String> {
    let cfg = crate::load(strategy)?;
    history_engine(database, &cfg, symbol)
}
pub(crate) fn history_engine(
    database: &Path,
    cfg: &crate::model::Strategy,
    symbol: &str,
) -> Result<Engine, String> {
    let mut engine = Engine::default();
    let time = chrono::DateTime::from_timestamp(now(), 0)
        .ok_or("invalid system time")?
        .to_rfc3339();
    let report = crate::data::inspect_symbol(database, &time, cfg, symbol)?;
    for (name, tf) in [
        ("W", Timeframe::W),
        ("D", Timeframe::D),
        ("H4", Timeframe::H4),
    ] {
        let bars = report["bars"][name]
            .as_array()
            .ok_or("invalid history context")?;
        let mut source = None;
        for b in bars {
            let current = b["source"].as_str().ok_or("missing history source")?;
            if source.is_some_and(|s| s != current) {
                return Err("mixed historical sources".into());
            }
            source = Some(current);
            let end = chrono::DateTime::parse_from_rfc3339(
                b["end_utc"].as_str().ok_or("invalid history time")?,
            )
            .map_err(|_| "invalid history time")?
            .timestamp();
            let price = |key: &str| b[key].as_i64().ok_or("invalid history price");
            engine.seed(
                tf,
                Candle {
                    end,
                    open: price("open")?,
                    high: price("high")?,
                    low: price("low")?,
                    close: price("close")?,
                },
                cfg,
            )?;
        }
    }
    Ok(engine)
}
pub fn prepare_state(database: &Path, strategy: &Path) -> Result<String, String> {
    if !database.is_file() {
        return Err("existing database required".into());
    }
    let cfg = crate::load(strategy)?;
    let mut runtime = Runtime::open_file(database, strategy, Engine::default())?;
    for item in &cfg.instruments {
        runtime.seed_instrument(
            &item.symbol,
            initial_engine(database, strategy, &item.symbol)?,
        )?;
    }
    Ok(format!(
        "Existing SQLite updated in place; rules loaded from JSON; {} instruments.",
        cfg.instruments.len()
    ))
}
fn image_mime(bytes: &[u8]) -> Result<String, Failure> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Ok("image/png".into())
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Ok("image/jpeg".into())
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Ok("image/gif".into())
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        Ok("image/webp".into())
    } else {
        Err(Failure::Invalid)
    }
}
fn analyze(
    c: &Settings,
    comment: &Comment,
    signal: Value,
    rules: Value,
) -> Result<(Analysis, Value), Failure> {
    analyze_at(c, comment, signal, rules, TELEGRAM, OPENAI)
}
fn analyze_at(
    c: &Settings,
    comment: &Comment,
    signal: Value,
    rules: Value,
    telegram: &str,
    openai: &str,
) -> Result<(Analysis, Value), Failure> {
    let api = Api::new()?;
    let dir = c
        .database
        .parent()
        .unwrap_or(Path::new("."))
        .join("attachments");
    std::fs::create_dir_all(&dir).map_err(|_| Failure::Invalid)?;
    let mut images = vec![];
    for id in &comment.files {
        let index = dir.join(format!("{:x}.ref", Sha256::digest(id.as_bytes())));
        let bytes = if let Ok(hash) = std::fs::read_to_string(&index) {
            if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(Failure::Invalid);
            }
            std::fs::read(dir.join(hash)).map_err(|_| Failure::Invalid)?
        } else {
            let info = api.telegram(
                telegram,
                &c.telegram_token,
                "getFile",
                &json!({"file_id":id}),
            )?;
            let bytes = api.file(
                telegram,
                &c.telegram_token,
                info["file_path"].as_str().ok_or(Failure::Invalid)?,
            )?;
            image_mime(&bytes)?;
            let hash = format!("{:x}", Sha256::digest(&bytes));
            std::fs::write(dir.join(&hash), &bytes).map_err(|_| Failure::Invalid)?;
            std::fs::write(&index, hash).map_err(|_| Failure::Invalid)?;
            bytes
        };
        images.push(Image {
            mime: image_mime(&bytes)?,
            bytes,
        });
    }
    let body = protocols::vision_request(&c.model, c.max_tokens, comment, &signal, &rules, &images)
        .map_err(|_| Failure::Invalid)?;
    let response = api.responses(openai, &c.openai_key, &body)?;
    let analysis = protocols::parse_analysis(&response).map_err(|_| Failure::Invalid)?;
    Ok((analysis, response["usage"].clone()))
}
enum Event {
    Tick(Tick),
    FeedDown,
    Updates(Result<Value, Failure>),
    Sent(String, Result<Value, Failure>),
    Analyzed(i64, Result<(Analysis, Value), Failure>),
    Market(String, Result<Value, Failure>),
}
fn feed_worker(tx: mpsc::SyncSender<Event>, symbols: Vec<String>) {
    loop {
        let result = (|| -> Result<(), ()> {
            let (mut socket, _) =
                tungstenite::connect("wss://biquote.io/hubs/tick").map_err(|_| ())?;
            match socket.get_mut() {
                tungstenite::stream::MaybeTlsStream::Plain(s) => {
                    s.set_read_timeout(Some(Duration::from_secs(30)))
                        .map_err(|_| ())?;
                }
                tungstenite::stream::MaybeTlsStream::Rustls(s) => {
                    s.sock
                        .set_read_timeout(Some(Duration::from_secs(30)))
                        .map_err(|_| ())?;
                }
                _ => return Err(()),
            }
            socket
                .send(tungstenite::Message::Text(
                    "{\"protocol\":\"json\",\"version\":1}\u{1e}".into(),
                ))
                .map_err(|_| ())?;
            let handshake = socket.read().map_err(|_| ())?;
            feed::parse_frames(handshake.to_text().map_err(|_| ())?).map_err(|_| ())?;
            let subscription =
                json!({"type":1,"invocationId":"1","target":"Subscribe","arguments":[symbols]});
            socket
                .send(tungstenite::Message::Text(format!("{subscription}\u{1e}")))
                .map_err(|_| ())?;
            let mut heartbeat = Instant::now();
            loop {
                let message = socket.read().map_err(|_| ())?;
                if message.is_close() {
                    return Err(());
                }
                if let tungstenite::Message::Text(text) = message {
                    for tick in feed::parse_frames(&text).map_err(|_| ())? {
                        if symbols.contains(&tick.symbol) {
                            tx.send(Event::Tick(tick)).map_err(|_| ())?;
                        }
                    }
                }
                if heartbeat.elapsed() >= Duration::from_secs(15) {
                    socket
                        .send(tungstenite::Message::Text("{\"type\":6}\u{1e}".into()))
                        .map_err(|_| ())?;
                    heartbeat = Instant::now();
                }
            }
        })();
        if result.is_err() && tx.send(Event::FeedDown).is_err() {
            return;
        }
        thread::sleep(Duration::from_secs(5));
    }
}
pub fn run(path: &Path) -> Result<String, String> {
    let c = Settings::load(path)?;
    let cfg = crate::load(&c.strategy)?;
    let symbols: Vec<String> = cfg.instruments.iter().map(|i| i.symbol.clone()).collect();
    let mut runtime = Runtime::open_file(&c.database, &c.strategy, Engine::default())?;
    for symbol in &symbols {
        runtime.seed_instrument(symbol, initial_engine(&c.database, &c.strategy, symbol)?)?;
    }
    runtime.started(now())?;
    let (tx, rx) = mpsc::sync_channel(1024);
    let feed_tx = tx.clone();
    let subscribed = symbols.clone();
    thread::spawn(move || feed_worker(feed_tx, subscribed));
    let (mut poll_busy, mut send_busy, mut api_busy, mut market_busy) =
        (false, false, false, false);
    let start = Instant::now();
    let (mut poll_at, mut send_at, mut api_at, mut market_at) = (start, start, start, start);
    let mut market_closed: std::collections::BTreeMap<String, bool> =
        symbols.iter().map(|s| (s.clone(), false)).collect();
    let mut last_tick: std::collections::BTreeMap<String, Instant> = symbols
        .iter()
        .map(|s| (s.clone(), Instant::now()))
        .collect();
    println!("Service started; credentials are not logged.");
    loop {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(Event::Tick(t)) => {
                let age = now().saturating_sub(t.millis / 1000);
                if age >= 0 && age <= c.stale_seconds {
                    last_tick.insert(t.symbol.clone(), Instant::now());
                    market_closed.insert(t.symbol.clone(), false);
                    let symbol = t.symbol.clone();
                    match runtime.tick_logged(t, true) {
                        Ok(logs) => {
                            for line in logs {
                                println!("{line}");
                            }
                        }
                        Err(e) if e == "out of order tick" => continue,
                        Err(e) => {
                            eprintln!("{symbol}: {e}");
                            runtime.disconnected_symbol(&symbol)?;
                        }
                    }
                }
            }
            Ok(Event::FeedDown) => {
                for symbol in &symbols {
                    if !market_closed[symbol] {
                        runtime.disconnected_symbol(symbol)?;
                    }
                }
            }
            Ok(Event::Updates(result)) => {
                poll_busy = false;
                poll_at = Instant::now() + Duration::from_secs(if result.is_ok() { 0 } else { 30 });
                match result {
                    Ok(v) => {
                        let updates = v.as_array().ok_or("invalid Telegram updates")?;
                        for u in updates {
                            runtime.update(u, c.chat)?;
                        }
                    }
                    Err(e) => eprintln!("Telegram polling: {e}"),
                }
            }
            Ok(Event::Sent(id, result)) => {
                send_busy = false;
                send_at = Instant::now() + Duration::from_secs(if result.is_ok() { 0 } else { 30 });
                match result {
                    Ok(v) => runtime.delivered(
                        &id,
                        v["message_id"]
                            .as_i64()
                            .ok_or("missing Telegram message ID")?,
                    )?,
                    Err(e) => eprintln!("Telegram delivery: {e}"),
                }
            }
            Ok(Event::Analyzed(id, result)) => {
                api_busy = false;
                api_at = Instant::now() + Duration::from_secs(if result.is_ok() { 0 } else { 60 });
                match result {
    Ok((a,usage))=>{runtime.analysis(id,&a,&usage)?;},
    Err(Failure::Invalid)=>runtime.analysis(id,&Analysis{action:"clarify".into(),explanation:"Не удалось разобрать комментарий или изображение. Правила сохранены. Пришлите уточнение ответом на это сообщение.".into(),changes:vec![]},&json!({}))?,
    Err(e)=>{eprintln!("OpenAI job: {e}");runtime.api_outage()?;}
   }
            }
            Ok(Event::Market(symbol, result)) => {
                market_busy = false;
                market_at = Instant::now() + Duration::from_secs(60);
                if let Ok(v) = result {
                    market_closed.insert(symbol, v["marketState"] == "closed");
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err("service workers disconnected".into()),
        }
        let time = Instant::now();
        if let Some(line) = runtime.status_log(start.elapsed()) {
            println!("{line}");
        }
        for symbol in &symbols {
            if !market_closed[symbol]
                && last_tick[symbol].elapsed().as_secs() > c.stale_seconds as u64
            {
                runtime.disconnected_symbol(symbol)?;
            }
        }
        if !market_busy && time >= market_at {
            market_busy = true;
            let tx = tx.clone();
            let instruments = symbols.clone();
            thread::spawn(move || {
                for symbol in instruments {
                    let result = Api::new().and_then(|api| api.market(&symbol));
                    let _ = tx.send(Event::Market(symbol, result));
                }
            });
        }
        if !poll_busy && time >= poll_at {
            poll_busy = true;
            let tx = tx.clone();
            let token = c.telegram_token.clone();
            let offset = runtime.offset();
            thread::spawn(move || {
                let result = Api::new().and_then(|api| {
                    api.telegram(
                        TELEGRAM,
                        &token,
                        "getUpdates",
                        &json!({"offset":offset,"timeout":20,"allowed_updates":["message"]}),
                    )
                });
                let _ = tx.send(Event::Updates(result));
            });
        }
        if !send_busy && time >= send_at {
            if let Some(out) = runtime.pending()?.into_iter().next() {
                send_busy = true;
                let tx = tx.clone();
                let token = c.telegram_token.clone();
                let chat = c.chat;
                thread::spawn(move || {
                    let mut body = json!({"chat_id":chat,"text":out.payload["text"]});
                    if let Some(reply) = out.payload["reply_to"].as_i64() {
                        body["reply_parameters"] =
                            json!({"message_id":reply,"allow_sending_without_reply":true});
                    }
                    let result = Api::new()
                        .and_then(|api| api.telegram(TELEGRAM, &token, "sendMessage", &body));
                    let _ = tx.send(Event::Sent(out.id, result));
                });
            }
        }
        if !api_busy && time >= api_at {
            if let Some(comment) = runtime.next_comment() {
                api_busy = true;
                let tx = tx.clone();
                let settings = c.clone();
                let rules = runtime.rules();
                let signal = runtime.comment_context(&comment);
                thread::spawn(move || {
                    let result = analyze(&settings, &comment, signal, rules);
                    let _ = tx.send(Event::Analyzed(comment.update_id, result));
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    #[test]
    fn image_job_downloads_preserves_and_sends_actual_bytes_with_structured_response() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let response = json!({"status":"completed","usage":{"input_tokens":20,"output_tokens":10},"output":[{"type":"message","content":[{"type":"output_text","text":json!({"action":"no_change","explanation":"Изменений нет","changes":[]}).to_string()}]}]});
        let responses = vec![
            br#"{"ok":true,"result":{"file_path":"photos/test.png"}}"#.to_vec(),
            b"\x89PNG\r\n\x1a\nfixture".to_vec(),
            response.to_string().into_bytes(),
        ];
        let server = thread::spawn(move || {
            let mut requests = vec![];
            for response in responses {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                let mut bytes = vec![];
                let mut buf = [0; 8192];
                loop {
                    let n = stream.read(&mut buf).unwrap();
                    assert!(n > 0);
                    bytes.extend_from_slice(&buf[..n]);
                    if let Some(i) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        let h = String::from_utf8_lossy(&bytes[..i]).to_lowercase();
                        let n: usize = h
                            .lines()
                            .find_map(|l| l.strip_prefix("content-length: "))
                            .unwrap_or("0")
                            .parse()
                            .unwrap();
                        if bytes.len() >= i + 4 + n {
                            break;
                        }
                    }
                }
                write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    response.len()
                )
                .unwrap();
                stream.write_all(&response).unwrap();
                requests.push(bytes);
            }
            requests
        });
        let dir =
            std::env::temp_dir().join(format!("forex-image-job-{}-{}", std::process::id(), now()));
        std::fs::create_dir_all(&dir).unwrap();
        let cfg = Settings {
            chat: -1,
            model: "fixture-model".into(),
            telegram_token: "fixture-token".into(),
            openai_key: "fixture-key".into(),
            max_tokens: 1000,
            database: dir.join("db.sqlite"),
            strategy: "unused".into(),
            stale_seconds: 120,
        };
        let comment = Comment {
            update_id: 1,
            message_id: 2,
            chat_id: -1,
            author_id: 11,
            signal_id: "s".into(),
            text: "Что на изображении?".into(),
            files: vec!["fixture-image".into()],
        };
        let (a, usage) =
            analyze_at(&cfg, &comment, json!({"id":"s"}), json!({}), &base, &base).unwrap();
        assert_eq!(a.action, "no_change");
        assert_eq!(usage["input_tokens"], 20);
        let requests = server.join().unwrap();
        let request = String::from_utf8(requests[2].clone()).unwrap();
        let body: Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
        use base64::Engine as _;
        let encoded = body["input"][0]["content"][1]["image_url"]
            .as_str()
            .unwrap()
            .strip_prefix("data:image/png;base64,")
            .unwrap();
        assert_eq!(
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .unwrap(),
            b"\x89PNG\r\n\x1a\nfixture"
        );
        let hash = format!("{:x}", Sha256::digest(b"\x89PNG\r\n\x1a\nfixture"));
        assert_eq!(
            std::fs::read(dir.join("attachments").join(hash)).unwrap(),
            b"\x89PNG\r\n\x1a\nfixture"
        );
    }
}
