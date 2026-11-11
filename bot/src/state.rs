use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value;
use std::path::Path;
#[derive(Debug, Clone, Copy)]
pub enum Kind {
    Plan,
    Signal,
    Comment,
    Job,
    Runtime,
}
impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Plan => "plan",
            Self::Signal => "signal",
            Self::Comment => "comment",
            Self::Job => "job",
            Self::Runtime => "runtime",
        }
    }
}
pub struct Entity {
    pub kind: Kind,
    pub key: String,
    pub value: Value,
}
pub struct Outbound {
    pub id: String,
    pub payload: Value,
}
pub struct Store {
    db: Connection,
}
fn err(_: rusqlite::Error) -> String {
    "state database operation failed".into()
}
impl Store {
    pub fn retire_level_revisions(&mut self, snapshot: &Value) -> Result<(), String> {
        let tx = self.db.transaction().map_err(err)?;
        tx.execute("UPDATE outbox SET delivered=1 WHERE delivered=0 AND (json_type(payload,'$.revision') IS NOT NULL OR id LIKE 'revision:%' OR id LIKE 'reconfigure:%')", []).map_err(err)?;
        tx.execute("INSERT INTO entities(kind,key,payload) VALUES('runtime','coordinator',?1) ON CONFLICT(kind,key) DO UPDATE SET payload=excluded.payload", [snapshot.to_string()]).map_err(err)?;
        tx.commit().map_err(err)
    }

    pub fn save_runtime(&mut self, v: &Value) -> Result<(), String> {
        self.db.execute("INSERT INTO entities(kind,key,payload) VALUES('runtime','coordinator',?1) ON CONFLICT(kind,key) DO UPDATE SET payload=excluded.payload",[v.to_string()]).map_err(err)?;
        Ok(())
    }

    pub fn acknowledge(
        &mut self,
        id: &str,
        event: &Value,
        entities: &[Entity],
        message: &str,
    ) -> Result<bool, String> {
        self.apply_inner(id, event, entities, &[], Some(message))
    }

    pub fn open(path: &Path) -> Result<Self, String> {
        Ok(Self {
            db: crate::data::open(path)?,
        })
    }
    pub fn apply(
        &mut self,
        id: &str,
        event: &Value,
        entities: &[Entity],
        messages: &[Outbound],
    ) -> Result<bool, String> {
        self.apply_inner(id, event, entities, messages, None)
    }
    fn apply_inner(
        &mut self,
        id: &str,
        event: &Value,
        entities: &[Entity],
        messages: &[Outbound],
        acknowledge: Option<&str>,
    ) -> Result<bool, String> {
        if id.is_empty()
            || entities.iter().any(|e| e.key.is_empty())
            || messages.iter().any(|m| m.id.is_empty())
        {
            return Err("empty event/entity/message identifier".into());
        }
        let payload = event.to_string();
        let tx = self.db.transaction().map_err(err)?;
        let old: Option<String> = tx
            .query_row("SELECT payload FROM inbox WHERE id=?1", [id], |r| r.get(0))
            .optional()
            .map_err(err)?;
        if let Some(old) = old {
            return if old == payload {
                Ok(false)
            } else {
                Err("conflicting event identifier".into())
            };
        }
        tx.execute("INSERT INTO inbox VALUES(?1,?2)", params![id, payload])
            .map_err(err)?;
        for e in entities {
            tx.execute("INSERT INTO entities(kind,key,payload) VALUES(?1,?2,?3) ON CONFLICT(kind,key) DO UPDATE SET payload=excluded.payload",params![e.kind.name(),e.key,e.value.to_string()]).map_err(err)?;
        }
        for m in messages {
            tx.execute(
                "INSERT INTO outbox(id,payload) VALUES(?1,?2)",
                params![m.id, m.payload.to_string()],
            )
            .map_err(err)?;
        }
        if let Some(message) = acknowledge {
            if tx
                .execute("UPDATE outbox SET delivered=1 WHERE id=?1", [message])
                .map_err(err)?
                != 1
            {
                return Err("unknown message identifier".into());
            }
        }
        tx.commit().map_err(err)?;
        Ok(true)
    }
    pub fn entities(&self, kind: Kind) -> Result<Vec<Entity>, String> {
        let mut stmt = self
            .db
            .prepare("SELECT key,payload FROM entities WHERE kind=?1 ORDER BY seq")
            .map_err(err)?;
        let rows = stmt
            .query_map([kind.name()], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(err)?;
        rows.map(|r| {
            let (key, payload) = r.map_err(err)?;
            Ok(Entity {
                kind,
                key,
                value: serde_json::from_str(&payload).map_err(|_| "corrupt stored entity")?,
            })
        })
        .collect()
    }
    pub fn pending(&self) -> Result<Vec<Outbound>, String> {
        let mut stmt = self
            .db
            .prepare("SELECT id,payload FROM outbox WHERE delivered=0 ORDER BY seq")
            .map_err(err)?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map_err(err)?;
        rows.map(|r| {
            let (id, payload) = r.map_err(err)?;
            Ok(Outbound {
                id,
                payload: serde_json::from_str(&payload).map_err(|_| "corrupt outbox")?,
            })
        })
        .collect()
    }
    pub fn delivered(&mut self, id: &str) -> Result<(), String> {
        if self
            .db
            .execute("UPDATE outbox SET delivered=1 WHERE id=?1", [id])
            .map_err(err)?
            != 1
        {
            return Err("unknown message identifier".into());
        }
        Ok(())
    }
}
