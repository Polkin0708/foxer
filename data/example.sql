-- Example: empty Forex SQLite schema, no user data.
-- Apply only to a new database; this is not a migration.
BEGIN TRANSACTION;
CREATE TABLE entities(seq INTEGER PRIMARY KEY AUTOINCREMENT,kind TEXT NOT NULL,key TEXT NOT NULL,payload TEXT NOT NULL,UNIQUE(kind,key));
CREATE TABLE imports(symbol TEXT NOT NULL,digest TEXT NOT NULL,report TEXT NOT NULL,PRIMARY KEY(symbol,digest));
CREATE TABLE inbox(id TEXT PRIMARY KEY,payload TEXT NOT NULL);
CREATE TABLE minutes(symbol TEXT NOT NULL,source TEXT NOT NULL,timestamp INTEGER NOT NULL,payload TEXT NOT NULL,PRIMARY KEY(symbol,source,timestamp));
CREATE TABLE outbox(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,payload TEXT NOT NULL,delivered INTEGER NOT NULL DEFAULT 0 CHECK(delivered IN (0,1)));
PRAGMA application_id = 1180190513;
PRAGMA user_version = 4;
COMMIT;
