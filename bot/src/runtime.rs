use crate::{
    engine::Engine,
    feed::{Aggregator, Tick},
    model::Strategy,
    protocols::{self, Analysis, Comment},
    state::{Entity, Kind, Outbound, Store},
    swing::{Kind as SwingKind, Swing, SwingConfig, Trade as SwingTrade, TradeState},
    trading::{Side, Signal},
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, VecDeque},
    path::Path,
};
#[derive(Clone, Serialize, Deserialize)]
struct Snapshot {
    #[serde(default)]
    calendar_version: u32,
    #[serde(default)]
    hash_version: u32,
    markets: BTreeMap<String, Market>,
    rules_hash: String,
    #[serde(default)]
    operation: Option<RuleOperation>,
    known: BTreeMap<i64, String>,
    #[serde(default)]
    texts: BTreeMap<i64, String>,
    #[serde(default)]
    reply_context: BTreeMap<i64, String>,
    offset: i64,
    queue: VecDeque<Comment>,
    api_down: bool,
    sequence: u64,
    usage: Value,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Market {
    engine: Engine,
    aggregator: Aggregator,
    feed_down: bool,
    #[serde(default)]
    swing: Option<SwingMarket>,
}
/// Swing rules of one instrument (requirements: Правила «свинг» в работе бота).
#[derive(Clone, Serialize, Deserialize)]
struct SwingMarket {
    config: Value,
    rules: Swing,
    /// Last published event per open trade.
    published: BTreeMap<String, String>,
}
fn swing_id(symbol: &str, t: &SwingTrade) -> String {
    let kind = match t.kind {
        SwingKind::Trend => "trend",
        SwingKind::Correction => "correction",
        SwingKind::Range => "range",
    };
    format!("{symbol}:swing:{kind}:{}:{}", t.dir, t.signal_t)
}
fn swing_event(t: &SwingTrade, previous: Option<&str>) -> &'static str {
    match t.state {
        TradeState::Pending => "Лимитная заявка",
        TradeState::Active if previous == Some("Лимитная заявка") => {
            "Заявка исполнена"
        }
        TradeState::Active if t.fill_t.is_some_and(|f| f != t.signal_t) => "Заявка исполнена",
        TradeState::Active => "Сделка открыта",
        TradeState::Stop => "Стоп",
        TradeState::Target => "Тейк",
        TradeState::Cancelled => "Заявка снята",
        TradeState::ReferenceExit => "Закрыта по индексу доллара",
    }
}
pub fn swing_text(symbol: &str, t: &SwingTrade, event: &str, time: Option<i64>) -> String {
    let pair = format!("{}/{}", &symbol[..3], &symbol[3..]);
    let side = if t.dir > 0 {
        "покупка"
    } else {
        "продажа"
    };
    let kind = match t.kind {
        SwingKind::Trend => "тренд",
        SwingKind::Correction => "коррекция",
        SwingKind::Range => "боковик",
    };
    let digits = if symbol.ends_with("JPY") { 3 } else { 5 };
    let p = |v: f64| format!("{v:.digits$}");
    let order = if t.fill_t == Some(t.signal_t) {
        "по рынку"
    } else {
        "лимит"
    };
    let when = time
        .and_then(|time| chrono::DateTime::from_timestamp(time, 0))
        .map(|t| format!("\n{}", t.to_rfc3339()))
        .unwrap_or_default();
    let rr = (t.tp - t.entry).abs() / (t.entry - t.stop).abs();
    let mut text = format!(
        "{pair} · {side} · {kind}\n{event}\nВход {} ({order}) · стоп {} · тейк {} · RR {rr:.2}{when}",
        p(t.entry),
        p(t.stop),
        p(t.tp)
    );
    if let Some(r) = t.r {
        text.push_str(&format!("\nРезультат {r:+.2}R"));
    }
    text
}
#[derive(Clone, Serialize, Deserialize)]
struct RuleOperation {
    id: i64,
    before: String,
    after: String,
}
fn hash(v: &Value) -> String {
    format!("{:x}", Sha256::digest(v.to_string().as_bytes()))
}
fn calculation_hash(v: &Value) -> Result<String, String> {
    Ok(hash(&forex_core::document::calculations(v)?))
}
pub struct Runtime {
    store: Store,
    rules: Value,
    rules_path: Option<std::path::PathBuf>,
    s: Snapshot,
    last_status: std::time::Duration,
    startup_announced: bool,
}
fn value<T: Serialize>(v: &T) -> Result<Value, String> {
    serde_json::to_value(v).map_err(|_| "snapshot serialization failed".into())
}
fn message(id: String, text: String, signal: Option<&str>, reply: Option<i64>) -> Outbound {
    Outbound {
        id,
        payload: json!({"text":text,"signal_id":signal,"reply_to":reply}),
    }
}
pub fn signal_text(s: &Signal) -> Result<String, String> {
    let symbol =
        s.id.split_once(':')
            .filter(|(symbol, _)| crate::valid_symbol(symbol))
            .map(|(symbol, _)| symbol)
            .ok_or("Не удалось определить пару сигнала: отсутствует или некорректен префикс ID")?;
    let pair = format!("{}/{}", &symbol[..3], &symbol[3..]);
    let side = if s.side == Side::Long {
        "покупка"
    } else {
        "продажа"
    };
    let risk = (s.entry as f64 - s.stop as f64).abs();
    let rr = (s.take_profit as f64 - s.entry as f64).abs() / risk;
    let p = |n: i64| format!("{}.{:06}", n / 1_000_000, n % 1_000_000);
    let when = chrono::DateTime::from_timestamp(s.last_quote_time, 0)
        .map(|t| t.to_rfc3339())
        .unwrap_or_default();
    Ok(format!("{pair} · {side}\n{when}\nВход {} · стоп {} · цель {} · RR {rr:.2}\nСостояние {:?}\nПравило {}",p(s.entry),p(s.stop),p(s.take_profit),s.state,s.id.split(':').find(|part| !crate::valid_symbol(part)).unwrap_or("")))
}
impl Runtime {
    pub fn started(&mut self, time: i64) -> Result<(), String> {
        if self.startup_announced {
            return Ok(());
        }
        let time = chrono::DateTime::from_timestamp(time, 0)
            .ok_or("invalid startup time")?
            .to_rfc3339();
        let mut s = self.s.clone();
        s.sequence = s.sequence.checked_add(1).ok_or("event sequence overflow")?;
        let id = format!("startup:{}", s.sequence);
        self.commit(
            &id,
            json!({"started_at":time}),
            s,
            vec![message(
                id.clone(),
                format!("Бот запущен. Время запуска: {time}"),
                None,
                None,
            )],
        )?;
        self.startup_announced = true;
        Ok(())
    }

    pub fn status_log(&mut self, elapsed: std::time::Duration) -> Option<String> {
        if elapsed.saturating_sub(self.last_status) < std::time::Duration::from_secs(300) {
            return None;
        }
        self.last_status = elapsed;
        Some(
            self.s
                .markets
                .iter()
                .map(|(symbol, m)| {
                    let last = m
                        .aggregator
                        .last_quote_millis()
                        .and_then(chrono::DateTime::from_timestamp_millis)
                        .map(|t| t.to_rfc3339())
                        .unwrap_or_else(|| "нет".into());
                    format!(
                        "{symbol}: Работаю; последняя котировка: {last}; ожидающих планов: {}",
                        m.engine.waiting_count()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }
    pub fn comment_context(&self, c: &Comment) -> Value {
        json!({"signal":self.signal_context(&c.signal_id),"replied_message":self.s.reply_context.get(&c.update_id)})
    }

    pub fn open(path: &Path, rules: Value, engine: Engine) -> Result<Self, String> {
        let c: Strategy =
            serde_json::from_value(rules.clone()).map_err(|_| "invalid rules JSON")?;
        crate::validate(&c)?;
        let rules_hash = calculation_hash(&rules)?;
        let mut store = Store::open(path)?;
        let stored = store
            .entities(Kind::Runtime)?
            .into_iter()
            .find(|e| e.key == "coordinator");
        let mut s = if let Some(e) = stored {
            serde_json::from_value::<Snapshot>(e.value).map_err(|_| "invalid runtime snapshot")?
        } else {
            Snapshot {
                calendar_version: 4,
                hash_version: 2,
                markets: BTreeMap::new(),
                rules_hash: rules_hash.clone(),
                operation: None,
                known: BTreeMap::new(),
                texts: BTreeMap::new(),
                reply_context: BTreeMap::new(),
                offset: 0,
                queue: VecDeque::new(),
                api_down: false,
                sequence: 0,
                usage: Value::Null,
            }
        };
        if s.hash_version > 2 {
            return Err("unsupported runtime hash version".into());
        }
        if s.hash_version != 2 && s.operation.is_some() {
            return Err("unfinished legacy rule operation; restore with the previous application before upgrading".into());
        }
        let changed = s.hash_version != 2 || s.rules_hash != rules_hash;
        let old_calendar = s.calendar_version != 4;
        let mut recovery_messages = vec![];
        for item in &c.instruments {
            let m = s
                .markets
                .entry(item.symbol.clone())
                .or_insert_with(|| Market {
                    engine: if item.symbol == "EURUSD" {
                        engine.clone()
                    } else {
                        Engine::default()
                    },
                    ..Market::default()
                });
            m.aggregator.disconnected();
            if old_calendar {
                m.engine.infer_legacy_entries();
                let context = crate::service::history_engine(path, &c, &item.symbol)?;
                m.engine.replace_context(context);
                m.aggregator = Aggregator::default();
            }
            if changed && !old_calendar {
                for revised in m.engine.reconfigure(&c, item.price_tick)? {
                    let mut out = message(
                        format!("reconfigure:{}:{}", hash(&rules), revised.id),
                        format!("Требует изменения\n{}", signal_text(&revised)?),
                        Some(&revised.id),
                        None,
                    );
                    out.payload["revision"] = value(&revised)?;
                    out.payload["symbol"] = json!(item.symbol);
                    recovery_messages.push(out);
                }
            }
        }
        for (symbol, m) in &s.markets {
            if !c.instruments.iter().any(|i| &i.symbol == symbol)
                && (!m.engine.signals().is_empty() || m.engine.waiting_count() > 0)
            {
                return Err("cannot remove instrument with plans or signals".into());
            }
        }
        s.markets
            .retain(|symbol, _| c.instruments.iter().any(|i| &i.symbol == symbol));
        // A staged operation contains hashes only. Never apply its old patch on recovery.
        if let Some(op) = s.operation.take() {
            if hash(&rules) != op.before && s.queue.front().is_some_and(|c| c.update_id == op.id) {
                let comment = s.queue.pop_front().unwrap();
                let explanation = if hash(&rules) == op.after {
                    "Изменение сохранено в JSON; обработка восстановлена после перезапуска."
                } else {
                    "JSON изменён отдельно; прерванная правка не применяется повторно."
                };
                recovery_messages.push(message(
                    format!("analysis:{}", op.id),
                    explanation.into(),
                    Some(&comment.signal_id),
                    Some(comment.message_id),
                ));
            }
        }
        s.calendar_version = 4;
        s.rules_hash = rules_hash;
        s.hash_version = 2;
        if recovery_messages.is_empty() {
            store.save_runtime(&value(&s)?)?;
        } else {
            s.sequence += 1;
            store.apply(
                &format!("restore:{}:{}", s.rules_hash, s.sequence),
                &json!({"rules_hash":s.rules_hash}),
                &[Entity {
                    kind: Kind::Runtime,
                    key: "coordinator".into(),
                    value: value(&s)?,
                }],
                &recovery_messages,
            )?;
        }
        store.retire_level_revisions(&value(&s)?)?;
        Ok(Self {
            store,
            s,
            rules,
            rules_path: None,
            last_status: std::time::Duration::ZERO,
            startup_announced: false,
        })
    }
    pub fn open_file(path: &Path, file: &Path, engine: Engine) -> Result<Self, String> {
        let rules = forex_core::load_document(file)?;
        let mut r = Self::open(path, rules, engine)?;
        r.rules_path = Some(file.to_owned());
        Ok(r)
    }
    pub fn seed_instrument(&mut self, symbol: &str, engine: Engine) -> Result<(), String> {
        let m = self.s.markets.get_mut(symbol).ok_or("unknown instrument")?;
        if m.aggregator.last_quote_millis().is_none()
            && m.engine.waiting_count() == 0
            && m.engine.signals().is_empty()
        {
            m.engine = engine;
        }
        self.store.save_runtime(&value(&self.s)?)
    }
    fn commit(
        &mut self,
        id: &str,
        event: Value,
        s: Snapshot,
        messages: Vec<Outbound>,
    ) -> Result<(), String> {
        let mut parts = Vec::new();
        for m in messages {
            let text = m.payload["text"].as_str().ok_or("missing outgoing text")?;
            let mut chunks = Vec::new();
            let mut chunk = String::new();
            let mut units = 0;
            for ch in text.chars() {
                if units + ch.len_utf16() > 3500 {
                    chunks.push(chunk);
                    chunk = String::new();
                    units = 0;
                }
                chunk.push(ch);
                units += ch.len_utf16();
            }
            chunks.push(chunk);
            let count = chunks.len();
            for (i, text) in chunks.into_iter().enumerate() {
                let mut payload = m.payload.clone();
                payload["text"] = json!(text);
                if i + 1 < count {
                    payload
                        .as_object_mut()
                        .ok_or("invalid outbox payload")?
                        .remove("revision");
                }
                parts.push(Outbound {
                    id: if count == 1 {
                        m.id.clone()
                    } else {
                        format!("{}:part:{i}", m.id)
                    },
                    payload,
                });
            }
        }
        if self.store.apply(
            id,
            &event,
            &[Entity {
                kind: Kind::Runtime,
                key: "coordinator".into(),
                value: value(&s)?,
            }],
            &parts,
        )? {
            self.s = s;
        }
        Ok(())
    }
    pub fn link(&mut self, id: i64, signal: &str) -> Result<(), String> {
        let mut s = self.s.clone();
        s.known.insert(id, signal.into());
        self.commit(&format!("link:{id}"), json!(signal), s, vec![])
    }
    pub fn update(&mut self, u: &Value, chat: i64) -> Result<(), String> {
        let id = u["update_id"].as_i64().ok_or("missing update ID")?;
        if id < self.s.offset {
            return Ok(());
        }
        let mut s = self.s.clone();
        if let Some(c) = protocols::parse_comment(u, chat, &s.known)? {
            if let Some(reply) = u["message"]["reply_to_message"]["message_id"].as_i64() {
                if let Some(text) = s.texts.get(&reply) {
                    s.reply_context.insert(id, text.clone());
                }
            }
            s.texts.insert(c.message_id, c.text.clone());
            s.queue.push_back(c);
        }
        s.offset = id.checked_add(1).ok_or("update ID overflow")?;
        self.commit(&format!("telegram:{id}"), u.clone(), s, vec![])
    }
    pub fn offset(&self) -> i64 {
        self.s.offset
    }
    pub fn next_comment(&self) -> Option<Comment> {
        // Never race a later rule revision against delivery of earlier levels.
        if self
            .store
            .pending()
            .ok()?
            .iter()
            .any(|m| m.payload.get("revision").is_some())
        {
            return None;
        }
        self.s.queue.front().cloned()
    }
    pub fn rules(&self) -> Value {
        self.rules.clone()
    }
    pub fn signal_context(&self, id: &str) -> Value {
        self.s
            .markets
            .values()
            .flat_map(|m| m.engine.signals())
            .find(|s| s.id == id)
            .and_then(|s| value(&s).ok())
            .unwrap_or(Value::Null)
    }
    pub fn analysis(&mut self, id: i64, a: &Analysis, usage: &Value) -> Result<(), String> {
        let comment = self
            .s
            .queue
            .front()
            .filter(|c| c.update_id == id)
            .cloned()
            .ok_or("analysis is not FIFO head")?;
        let mut s = self.s.clone();
        let mut messages = vec![];
        let mut explanation = a.explanation.clone();
        let mut next_rules = self.rules.clone();
        match protocols::apply_changes(&self.rules,a) {
            Ok(rules) if rules!=self.rules => {
                let c:Strategy=serde_json::from_value(rules.clone()).map_err(|_|"invalid changed rules")?;
                let next_hash=calculation_hash(&rules)?;
                if next_hash != s.rules_hash {
                for item in &c.instruments {
                    let m=s.markets.get_mut(&item.symbol).ok_or("unknown instrument")?;
                    for revised in m.engine.reconfigure(&c,item.price_tick)? {
                        let mut out=message(format!("revision:{id}:{}",revised.id),format!("Требует изменения\n{}",signal_text(&revised)?),Some(&revised.id),None);
                        out.payload["revision"]=value(&revised)?;
                        out.payload["symbol"]=json!(item.symbol);
                        messages.push(out);
                    }
                }
                }
                let file=self.rules_path.as_ref().ok_or("rules file is required for changes")?;
                let op=RuleOperation{id,before:hash(&self.rules),after:hash(&rules)};
                let mut staged=self.s.clone();staged.operation=Some(op);
                self.store.save_runtime(&value(&staged)?)?;
                if let Err(e)=write_rules(file,&rules) {
                    self.store.save_runtime(&value(&self.s)?)?;
                    return Err(e);
                }
                s.rules_hash=next_hash;s.operation=None;next_rules=rules;
                explanation.push_str("\nИзменения сохранены в JSON.");
            },
            Ok(_)=>{},
            Err(_)=>explanation="Изменение не применено: ответ не соответствует поддерживаемым правилам. Уточните параметр и значение.".into(),
        }
        messages.insert(
            0,
            message(
                format!("analysis:{id}"),
                explanation,
                Some(&comment.signal_id),
                Some(comment.message_id),
            ),
        );
        s.queue.pop_front();
        s.api_down = false;
        s.usage = usage.clone();
        self.commit(
            &format!("analysis:{id}"),
            json!({"action":a.action,"usage":usage}),
            s,
            messages,
        )?;
        self.rules = next_rules;
        Ok(())
    }
    pub fn pending(&self) -> Result<Vec<Outbound>, String> {
        Ok(self
            .store
            .pending()?
            .into_iter()
            .filter(|o| {
                !o.payload
                    .as_object()
                    .is_some_and(|p| p.contains_key("revision"))
                    && !o.id.starts_with("revision:")
                    && !o.id.starts_with("reconfigure:")
            })
            .collect())
    }
    pub fn api_outage(&mut self) -> Result<(), String> {
        if self.s.api_down {
            return Ok(());
        }
        let mut s = self.s.clone();
        s.api_down = true;
        s.sequence += 1;
        let id = format!("api-down:{}", s.sequence);
        self.commit(&id,json!("api-down"),s,vec![message(id.clone(),"OpenAI недоступен. Комментарии сохранены; сигналы продолжаются по действующим правилам.".into(),None,None)])
    }
    pub fn disconnected(&mut self) -> Result<(), String> {
        for symbol in self.s.markets.keys().cloned().collect::<Vec<_>>() {
            self.disconnected_symbol(&symbol)?;
        }
        Ok(())
    }
    pub fn disconnected_symbol(&mut self, symbol: &str) -> Result<(), String> {
        let mut s = self.s.clone();
        let m = s.markets.get_mut(symbol).ok_or("unknown instrument")?;
        m.aggregator.disconnected();
        if m.feed_down {
            return Ok(());
        }
        m.feed_down = true;
        s.sequence += 1;
        let id = format!("feed-down:{symbol}:{}", s.sequence);
        self.commit(
            &id,
            json!({"symbol":symbol,"feed_down":true}),
            s,
            vec![message(
                id.clone(),
                format!("{symbol}: поток котировок недоступен. Новые сигналы приостановлены."),
                None,
                None,
            )],
        )
    }
    pub fn tick(&mut self, t: Tick, allow_new: bool) -> Result<(), String> {
        self.tick_logged(t, allow_new).map(|_| ())
    }
    pub fn tick_logged(&mut self, t: Tick, allow_new: bool) -> Result<Vec<String>, String> {
        if t.source != "upscale:index" {
            return Err("live runtime requires Upscale index ticks".into());
        }
        let event = value(&t)?;
        let id = format!("tick:{:x}", Sha256::digest(event.to_string().as_bytes()));
        let mut s = self.s.clone();
        let symbol = t.symbol.clone();
        let m = s.markets.get_mut(&symbol).ok_or("unknown instrument")?;
        let before = m.engine.signals();
        let mut messages = vec![];
        let cfg: Strategy =
            serde_json::from_value(self.rules.clone()).map_err(|_| "invalid stored rules")?;
        let batch =
            crate::processing::process(&mut m.engine, &mut m.aggregator, &t, &cfg, allow_new)?;
        let h4_end = batch
            .bars
            .iter()
            .find(|(tf, _)| *tf == crate::engine::Timeframe::H4)
            .map(|(_, b)| b.end);
        for (previous, current) in batch.weekly {
            let rule = match (previous, current) {
                (None, Some(_)) => "W_trend_new",
                (Some(_), None) => "W_trend_end",
                _ => "W_trend_flip",
            };
            let description = match rule {
                "W_trend_new" => &cfg.messages.w_trend_new,
                "W_trend_end" => &cfg.messages.w_trend_end,
                _ => &cfg.messages.w_trend_flip,
            };
            let direction = match current {
                Some(Side::Long) => "↑",
                Some(Side::Short) => "↓",
                None => "нет направления",
            };
            messages.push(message(
                format!("{id}:{rule}"),
                format!(
                    "{symbol}: {rule}: {}",
                    description.replace("↑/↓", direction)
                ),
                None,
                None,
            ));
        }
        if m.feed_down {
            m.feed_down = false;
            messages.push(message(
                format!("{id}:restored"),
                format!("{symbol}: Котировки восстановлены. Пропущенное движение цены не восстанавливается."),
                None,
                None,
            ));
        }
        if let Some(sw) = m.swing.as_mut() {
            let now = crate::swing::grid_time(t.millis / 1000);
            for (tf, bar) in crate::swing::rules_bars(&batch.all_bars) {
                sw.rules.close(tf, bar, now);
            }
            let price = t.price as f64 / 1_000_000.0;
            sw.rules.minute(now, price, price);
            let mut trades = sw.rules.drain_finished();
            trades.extend(sw.rules.trades().iter().cloned());
            for tr in &trades {
                let key = swing_id(&symbol, tr);
                let previous = sw.published.get(&key).cloned();
                let mut events = vec![];
                if previous.is_none()
                    && tr.state != TradeState::Pending
                    && tr.state != TradeState::Active
                {
                    events.push(if tr.fill_t == Some(tr.signal_t) {
                        "Сделка открыта"
                    } else {
                        "Лимитная заявка"
                    });
                }
                let event = swing_event(tr, previous.as_deref());
                if previous.as_deref() != Some(event) {
                    events.push(event);
                }
                for event in events {
                    messages.push(message(
                        format!("{id}:{key}:{event}"),
                        swing_text(&symbol, tr, event, Some(t.millis / 1000)),
                        Some(&key),
                        None,
                    ));
                }
                if matches!(tr.state, TradeState::Pending | TradeState::Active) {
                    sw.published.insert(key, event.to_owned());
                } else {
                    sw.published.remove(&key);
                }
            }
        }
        m.engine.namespace_signals(&symbol);
        for signal in m.engine.signals() {
            if m.swing.is_some() {
                break;
            }
            let previous = before.iter().find(|p| p.id == signal.id);
            if previous.is_none() || previous.is_some_and(|p| p.state != signal.state) {
                messages.push(message(
                    format!("{id}:{}", signal.id),
                    signal_text(&signal)?,
                    Some(&signal.id),
                    None,
                ));
            }
        }
        let mut logs = Vec::new();
        if let Some(end) = h4_end.filter(|_| allow_new) {
            let found = m
                .engine
                .signals()
                .iter()
                .filter(|signal| !before.iter().any(|old| old.id == signal.id))
                .count();
            let result = if found > 0 {
                format!("новых сигналов: {found}")
            } else {
                format!("новых сигналов нет: {}", m.engine.search_status(&cfg))
            };
            let time = chrono::DateTime::from_timestamp(end, 0)
                .ok_or("invalid H4 close time")?
                .to_rfc3339();
            logs.push(format!("{symbol}: H4 {time}; условия проверены; {result}"));
        }
        self.commit(&id, event, s, messages)?;
        Ok(logs)
    }
    /// Connects swing rules to an instrument. `warm` builds the rules from history and is
    /// called only when the instrument has no rules yet or its parameters changed.
    pub fn swing(
        &mut self,
        symbol: &str,
        config: Value,
        warm: impl FnOnce() -> Result<Swing, String>,
    ) -> Result<(), String> {
        let parsed: SwingConfig = serde_json::from_value(config.clone())
            .map_err(|e| format!("invalid swing rules for {symbol}: {e}"))?;
        if parsed.reference_exit.is_some() {
            return Err("swing reference_exit is not available in the live bot".into());
        }
        let current = self
            .s
            .markets
            .get(symbol)
            .ok_or("swing rules for an unknown instrument")?;
        if current.swing.as_ref().is_some_and(|sw| sw.config == config) {
            return Ok(());
        }
        let mut rules = warm()?;
        rules.drain_finished();
        let mut s = self.s.clone();
        s.sequence += 1;
        let tag = format!("swing:{symbol}:{}:{}", hash(&config), s.sequence);
        let mut published = BTreeMap::new();
        let mut messages = vec![];
        for tr in rules.trades() {
            let key = swing_id(symbol, tr);
            let event = if tr.state == TradeState::Pending {
                "Заявка до запуска"
            } else {
                "Открыта до запуска"
            };
            messages.push(message(
                format!("{tag}:{key}"),
                swing_text(symbol, tr, event, None),
                Some(&key),
                None,
            ));
            published.insert(key, swing_event(tr, None).to_owned());
        }
        s.markets
            .get_mut(symbol)
            .ok_or("swing rules for an unknown instrument")?
            .swing = Some(SwingMarket {
            config: config.clone(),
            rules,
            published,
        });
        self.commit(&tag, json!({"symbol":symbol,"swing":config}), s, messages)
    }
    pub fn swing_rules(&self, symbol: &str) -> Option<&Swing> {
        self.s
            .markets
            .get(symbol)?
            .swing
            .as_ref()
            .map(|sw| &sw.rules)
    }
    pub fn delivered(&mut self, id: &str, message_id: i64) -> Result<(), String> {
        let Some(out) = self.store.pending()?.into_iter().find(|m| m.id == id) else {
            return Ok(());
        };
        let mut s = self.s.clone();
        if let Some(text) = out.payload["text"].as_str() {
            s.texts.insert(message_id, text.into());
        }
        if let Some(signal) = out.payload["signal_id"].as_str() {
            s.known.insert(message_id, signal.into());
        }
        if self.store.acknowledge(
            &format!("delivered:{id}"),
            &json!({"message_id":message_id}),
            &[Entity {
                kind: Kind::Runtime,
                key: "coordinator".into(),
                value: value(&s)?,
            }],
            id,
        )? {
            self.s = s;
        }
        Ok(())
    }
}
fn write_rules(path: &Path, rules: &Value) -> Result<(), String> {
    use std::io::Write;
    let tmp = path.with_extension("json.pending");
    let bytes = serde_json::to_vec_pretty(rules).map_err(|_| "rules serialization failed")?;
    let mut file = std::fs::File::create(&tmp).map_err(|_| "cannot stage rules file")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "cannot persist rules file")?;
    std::fs::rename(&tmp, path).map_err(|_| "cannot publish rules file")?;
    std::fs::File::open(path.parent().unwrap_or(Path::new(".")))
        .and_then(|d| d.sync_all())
        .map_err(|_| "cannot sync rules directory")?;
    Ok(())
}
