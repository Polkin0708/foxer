"""Public SQLite example creates an empty, usable v3 schema."""
import sqlite3
import unittest
from pathlib import Path


class ExampleSchemaTests(unittest.TestCase):
    def test_empty_schema_and_constraints(self):
        sql = (Path(__file__).resolve().parents[1] / 'data/example.sql').read_text()
        with sqlite3.connect(':memory:') as db:
            db.executescript(sql)
            self.assertEqual(db.execute('PRAGMA application_id').fetchone()[0], 1180190513)
            self.assertEqual(db.execute('PRAGMA user_version').fetchone()[0], 3)
            tables = {r[0] for r in db.execute(
                "SELECT name FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'")}
            self.assertEqual(tables, {'minutes', 'imports', 'inbox', 'entities', 'outbox'})
            for table in tables:
                self.assertEqual(db.execute(f'SELECT count(*) FROM {table}').fetchone()[0], 0)
            db.execute("INSERT INTO minutes VALUES ('EURUSD','test',1,'{}')")
            db.execute("INSERT INTO minutes VALUES ('GBPUSD','test',1,'{}')")
            with self.assertRaises(sqlite3.IntegrityError):
                db.execute("INSERT INTO minutes VALUES ('EURUSD','test',1,'{}')")
            db.execute("INSERT INTO outbox(id,payload) VALUES ('test','{}')")
            self.assertEqual(db.execute('SELECT delivered FROM outbox').fetchone()[0], 0)
            with self.assertRaises(sqlite3.IntegrityError):
                db.execute("UPDATE outbox SET delivered=2")


if __name__ == '__main__':
    unittest.main()
