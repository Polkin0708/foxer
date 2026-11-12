use forex_bot::state::{Entity, Kind, Outbound, Store};
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};
static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let p = std::env::temp_dir().join(format!(
            "forex-state-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
    fn db(&self) -> PathBuf {
        self.0.join("state.sqlite")
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}
fn entity(key: &str) -> Entity {
    Entity {
        kind: Kind::Plan,
        key: key.into(),
        value: json!({"state":"active","stop":1100000}),
    }
}
fn message(id: &str) -> Outbound {
    Outbound {
        id: id.into(),
        payload: json!({"signal":"test","text":"signal"}),
    }
}
#[test]
fn state_and_pending_delivery_survive_restart_without_reapplying_event() {
    let f = Fixture::new();
    let mut s = Store::open(&f.db()).unwrap();
    assert!(s
        .apply(
            "quote:1",
            &json!({"price":1}),
            &[entity("p1")],
            &[message("m1")]
        )
        .unwrap());
    drop(s);
    let mut s = Store::open(&f.db()).unwrap();
    assert!(!s
        .apply(
            "quote:1",
            &json!({"price":1}),
            &[entity("p1")],
            &[message("m1")]
        )
        .unwrap());
    assert_eq!(s.entities(Kind::Plan).unwrap().len(), 1);
    assert_eq!(s.pending().unwrap().len(), 1);
    s.delivered("m1").unwrap();
    s.delivered("m1").unwrap();
    drop(s);
    assert!(Store::open(&f.db()).unwrap().pending().unwrap().is_empty());
}
#[test]
fn conflicting_event_id_does_not_overwrite_state() {
    let f = Fixture::new();
    let mut s = Store::open(&f.db()).unwrap();
    s.apply("e", &json!(1), &[entity("p")], &[]).unwrap();
    assert!(s.apply("e", &json!(2), &[entity("other")], &[]).is_err());
    assert_eq!(s.entities(Kind::Plan).unwrap().len(), 1);
}
#[test]
fn outbox_conflict_rolls_back_entities_and_inbox() {
    let f = Fixture::new();
    let mut s = Store::open(&f.db()).unwrap();
    s.apply("e1", &json!(1), &[], &[message("m")]).unwrap();
    assert!(s
        .apply("e2", &json!(2), &[entity("p")], &[message("m")])
        .is_err());
    assert!(s.entities(Kind::Plan).unwrap().is_empty());
    assert!(s
        .apply("e2", &json!(2), &[entity("p")], &[message("m2")])
        .unwrap());
}
#[test]
fn queue_order_and_updates_are_stable_across_restart() {
    let f = Fixture::new();
    let mut s = Store::open(&f.db()).unwrap();
    for (event, key) in [("1", "20"), ("2", "3"), ("3", "20")] {
        let mut e = entity(key);
        e.kind = Kind::Job;
        s.apply(event, &json!(event), &[e], &[]).unwrap();
    }
    drop(s);
    let s = Store::open(&f.db()).unwrap();
    let q = s.entities(Kind::Job).unwrap();
    assert_eq!(
        q.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
        vec!["20", "3"]
    );
}
#[test]
fn old_schema_is_not_migrated_or_modified() {
    let f = Fixture::new();
    let c = rusqlite::Connection::open(f.db()).unwrap();
    c.execute_batch("PRAGMA application_id=1180190513;PRAGMA user_version=1;CREATE TABLE minutes(source TEXT,timestamp INTEGER,payload TEXT,PRIMARY KEY(source,timestamp));CREATE TABLE imports(digest TEXT PRIMARY KEY,report TEXT);INSERT INTO imports VALUES('old','{}');").unwrap();
    drop(c);
    assert!(Store::open(&f.db()).is_err());
    let c = rusqlite::Connection::open(f.db()).unwrap();
    assert_eq!(
        c.query_row("SELECT digest FROM imports", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "old"
    );
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
}
#[test]
fn delivery_acknowledgement_commits_state_and_outbox_together() {
    let f = Fixture::new();
    let mut s = Store::open(&f.db()).unwrap();
    s.apply("create", &json!(1), &[], &[message("m")]).unwrap();
    assert!(s
        .acknowledge(
            "ack",
            &json!({"message_id":7}),
            &[entity("new-levels")],
            "m"
        )
        .unwrap());
    drop(s);
    let mut s = Store::open(&f.db()).unwrap();
    assert!(s.pending().unwrap().is_empty());
    assert_eq!(s.entities(Kind::Plan).unwrap().len(), 1);
    assert!(!s
        .acknowledge(
            "ack",
            &json!({"message_id":7}),
            &[entity("new-levels")],
            "m"
        )
        .unwrap());
}
#[test]
fn unknown_delivery_rolls_back_state_and_ack_event() {
    let f = Fixture::new();
    let mut s = Store::open(&f.db()).unwrap();
    assert!(s
        .acknowledge("ack", &json!(7), &[entity("new-levels")], "missing")
        .is_err());
    assert!(s.entities(Kind::Plan).unwrap().is_empty());
    s.apply("create", &json!(1), &[], &[message("missing")])
        .unwrap();
    assert!(s
        .acknowledge("ack", &json!(7), &[entity("new-levels")], "missing")
        .unwrap());
}
