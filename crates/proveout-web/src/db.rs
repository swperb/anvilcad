//! SQLite storage: users, sessions, monthly credit use, saved reports, projects and
//! machine profiles. One connection behind a mutex; every statement is short.

use crate::util::{now, period};
use proveout_core::plan::Plan;
use rusqlite::{Connection, OptionalExtension, params};
use serde::Serialize;
use std::sync::Mutex;

pub struct Db {
    conn: Mutex<Connection>,
}

pub type DbResult<T> = Result<T, String>;

fn e(err: rusqlite::Error) -> String {
    format!("database: {err}")
}

#[derive(Clone, Debug, Serialize)]
pub struct User {
    pub id: i64,
    pub email: String,
    #[serde(skip)]
    pub pw: String,
    pub plan: Plan,
    #[serde(skip)]
    pub stripe_customer: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub created: i64,
    pub bytes: i64,
    pub score: Option<i64>,
    pub project_id: Option<String>,
}

const SCHEMA: &str = "
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY,
    email TEXT NOT NULL UNIQUE,
    pw TEXT NOT NULL,
    plan TEXT NOT NULL DEFAULT 'free',
    stripe_customer TEXT,
    created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS sessions (
    token_hash TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS usage (
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    period TEXT NOT NULL,
    credits INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (user_id, period)
);
CREATE TABLE IF NOT EXISTS review_log (
    id INTEGER PRIMARY KEY,
    user_id INTEGER NOT NULL,
    at INTEGER NOT NULL,
    level TEXT NOT NULL,
    model TEXT NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    ok INTEGER NOT NULL,
    disclosure TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS projects (
    id TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    created INTEGER NOT NULL,
    data TEXT NOT NULL,
    bytes INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS reports (
    id TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    project_id TEXT,
    title TEXT NOT NULL,
    created INTEGER NOT NULL,
    score INTEGER NOT NULL,
    data TEXT NOT NULL,
    bytes INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS machines (
    id TEXT PRIMARY KEY,
    user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    created INTEGER NOT NULL,
    data TEXT NOT NULL,
    bytes INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS reports_user ON reports(user_id);
CREATE INDEX IF NOT EXISTS projects_user ON projects(user_id);
CREATE INDEX IF NOT EXISTS machines_user ON machines(user_id);
CREATE INDEX IF NOT EXISTS review_log_at ON review_log(at);
";

/// The three kinds of saved item. The table name comes only from here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Report,
    Project,
    Machine,
}

impl Kind {
    fn table(self) -> &'static str {
        match self {
            Kind::Report => "reports",
            Kind::Project => "projects",
            Kind::Machine => "machines",
        }
    }
    fn name_col(self) -> &'static str {
        match self {
            Kind::Report => "title",
            _ => "name",
        }
    }
}

impl Db {
    pub fn open(path: &str) -> DbResult<Db> {
        let conn = if path == ":memory:" { Connection::open_in_memory() } else { Connection::open(path) }
            .map_err(e)?;
        conn.execute_batch(SCHEMA).map_err(e)?;
        Ok(Db { conn: Mutex::new(conn) })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> DbResult<T> {
        let c = self.conn.lock().map_err(|_| "database lock poisoned".to_string())?;
        f(&c).map_err(e)
    }

    fn user_row(r: &rusqlite::Row) -> rusqlite::Result<User> {
        let plan: String = r.get(3)?;
        Ok(User {
            id: r.get(0)?,
            email: r.get(1)?,
            pw: r.get(2)?,
            plan: Plan::parse(&plan).unwrap_or(Plan::Free),
            stripe_customer: r.get(4)?,
        })
    }

    pub fn create_user(&self, email: &str, pw_hash: &str) -> DbResult<Option<i64>> {
        self.with(|c| {
            match c.execute(
                "INSERT INTO users (email, pw, created) VALUES (?1, ?2, ?3)",
                params![email, pw_hash, now()],
            ) {
                Ok(_) => Ok(Some(c.last_insert_rowid())),
                Err(rusqlite::Error::SqliteFailure(f, _))
                    if f.code == rusqlite::ErrorCode::ConstraintViolation =>
                {
                    Ok(None)
                }
                Err(err) => Err(err),
            }
        })
    }

    pub fn user_by_email(&self, email: &str) -> DbResult<Option<User>> {
        self.with(|c| {
            c.query_row(
                "SELECT id, email, pw, plan, stripe_customer FROM users WHERE email = ?1",
                [email],
                Self::user_row,
            )
            .optional()
        })
    }

    pub fn user(&self, id: i64) -> DbResult<Option<User>> {
        self.with(|c| {
            c.query_row(
                "SELECT id, email, pw, plan, stripe_customer FROM users WHERE id = ?1",
                [id],
                Self::user_row,
            )
            .optional()
        })
    }

    pub fn set_plan(&self, user_id: i64, plan: Plan, customer: Option<&str>) -> DbResult<()> {
        self.with(|c| {
            c.execute(
                "UPDATE users SET plan = ?1, stripe_customer = COALESCE(?2, stripe_customer) WHERE id = ?3",
                params![plan.as_str(), customer, user_id],
            )
            .map(|_| ())
        })
    }

    pub fn set_plan_by_customer(&self, customer: &str, plan: Plan) -> DbResult<usize> {
        self.with(|c| {
            c.execute(
                "UPDATE users SET plan = ?1 WHERE stripe_customer = ?2",
                params![plan.as_str(), customer],
            )
        })
    }

    pub fn set_plan_by_email(&self, email: &str, plan: Plan) -> DbResult<usize> {
        self.with(|c| c.execute("UPDATE users SET plan = ?1 WHERE email = ?2", params![plan.as_str(), email]))
    }

    pub fn create_session(&self, token_hash: &str, user_id: i64, ttl: i64) -> DbResult<()> {
        self.with(|c| {
            c.execute("DELETE FROM sessions WHERE expires < ?1", [now()])?;
            c.execute(
                "INSERT INTO sessions (token_hash, user_id, expires) VALUES (?1, ?2, ?3)",
                params![token_hash, user_id, now() + ttl],
            )
            .map(|_| ())
        })
    }

    pub fn session_user(&self, token_hash: &str) -> DbResult<Option<i64>> {
        self.with(|c| {
            c.query_row(
                "SELECT user_id FROM sessions WHERE token_hash = ?1 AND expires > ?2",
                params![token_hash, now()],
                |r| r.get(0),
            )
            .optional()
        })
    }

    pub fn delete_session(&self, token_hash: &str) -> DbResult<()> {
        self.with(|c| c.execute("DELETE FROM sessions WHERE token_hash = ?1", [token_hash]).map(|_| ()))
    }

    pub fn credits_used(&self, user_id: i64) -> DbResult<u32> {
        self.with(|c| {
            c.query_row(
                "SELECT credits FROM usage WHERE user_id = ?1 AND period = ?2",
                params![user_id, period(now())],
                |r| r.get::<_, i64>(0),
            )
            .optional()
            .map(|v| v.unwrap_or(0).max(0) as u32)
        })
    }

    /// Takes `credits` if the month's total stays within `limit`. Returns false when it
    /// would not. Done in one statement so two reviews cannot both take the last credit.
    pub fn reserve_credits(&self, user_id: i64, credits: u32, limit: u32) -> DbResult<bool> {
        self.with(|c| {
            let p = period(now());
            c.execute("INSERT OR IGNORE INTO usage (user_id, period, credits) VALUES (?1, ?2, 0)", params![user_id, p])?;
            let n = c.execute(
                "UPDATE usage SET credits = credits + ?1 WHERE user_id = ?2 AND period = ?3 AND credits + ?1 <= ?4",
                params![credits, user_id, p, limit],
            )?;
            Ok(n == 1)
        })
    }

    pub fn refund_credits(&self, user_id: i64, credits: u32) -> DbResult<()> {
        self.with(|c| {
            c.execute(
                "UPDATE usage SET credits = MAX(0, credits - ?1) WHERE user_id = ?2 AND period = ?3",
                params![credits, user_id, period(now())],
            )
            .map(|_| ())
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn log_review(
        &self,
        user_id: i64,
        level: &str,
        model: &str,
        input_tokens: u64,
        output_tokens: u64,
        ok: bool,
        disclosure: &str,
    ) -> DbResult<()> {
        self.with(|c| {
            c.execute(
                "INSERT INTO review_log (user_id, at, level, model, input_tokens, output_tokens, ok, disclosure) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![user_id, now(), level, model, input_tokens as i64, output_tokens as i64, ok, disclosure],
            )
            .map(|_| ())
        })
    }

    /// AI reviews started across all users in the last 24 hours.
    pub fn reviews_last_day(&self) -> DbResult<u32> {
        self.with(|c| {
            c.query_row("SELECT COUNT(*) FROM review_log WHERE at > ?1", [now() - 86_400], |r| {
                r.get::<_, i64>(0)
            })
            .map(|n| n as u32)
        })
    }

    pub fn storage_used(&self, user_id: i64) -> DbResult<u64> {
        self.with(|c| {
            c.query_row(
                "SELECT (SELECT COALESCE(SUM(bytes),0) FROM reports WHERE user_id = ?1) + \
                        (SELECT COALESCE(SUM(bytes),0) FROM projects WHERE user_id = ?1) + \
                        (SELECT COALESCE(SUM(bytes),0) FROM machines WHERE user_id = ?1)",
                [user_id],
                |r| r.get::<_, i64>(0),
            )
            .map(|n| n.max(0) as u64)
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn insert(
        &self,
        kind: Kind,
        id: &str,
        user_id: i64,
        name: &str,
        data: &str,
        score: Option<u32>,
        project_id: Option<&str>,
    ) -> DbResult<()> {
        self.with(|c| {
            let bytes = (data.len() + name.len()) as i64;
            match kind {
                Kind::Report => c.execute(
                    "INSERT INTO reports (id, user_id, project_id, title, created, score, data, bytes) \
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![id, user_id, project_id, name, now(), score.unwrap_or(0), data, bytes],
                ),
                _ => c.execute(
                    &format!(
                        "INSERT INTO {} (id, user_id, name, created, data, bytes) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                        kind.table()
                    ),
                    params![id, user_id, name, now(), data, bytes],
                ),
            }
            .map(|_| ())
        })
    }

    pub fn list(&self, kind: Kind, user_id: i64) -> DbResult<Vec<Item>> {
        self.with(|c| {
            let sql = match kind {
                Kind::Report => "SELECT id, title, created, bytes, score, project_id FROM reports WHERE user_id = ?1 \
                                 ORDER BY created DESC LIMIT 500"
                    .to_string(),
                _ => format!(
                    "SELECT id, {}, created, bytes, NULL, NULL FROM {} WHERE user_id = ?1 ORDER BY created DESC LIMIT 500",
                    kind.name_col(),
                    kind.table()
                ),
            };
            let mut st = c.prepare(&sql)?;
            let rows = st.query_map([user_id], |r| {
                Ok(Item {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    created: r.get(2)?,
                    bytes: r.get(3)?,
                    score: r.get(4)?,
                    project_id: r.get(5)?,
                })
            })?;
            rows.collect()
        })
    }

    pub fn get(&self, kind: Kind, user_id: i64, id: &str) -> DbResult<Option<String>> {
        self.with(|c| {
            c.query_row(
                &format!("SELECT data FROM {} WHERE id = ?1 AND user_id = ?2", kind.table()),
                params![id, user_id],
                |r| r.get(0),
            )
            .optional()
        })
    }

    pub fn delete(&self, kind: Kind, user_id: i64, id: &str) -> DbResult<bool> {
        self.with(|c| {
            c.execute(
                &format!("DELETE FROM {} WHERE id = ?1 AND user_id = ?2", kind.table()),
                params![id, user_id],
            )
            .map(|n| n > 0)
        })
    }

    pub fn delete_user(&self, user_id: i64) -> DbResult<()> {
        self.with(|c| c.execute("DELETE FROM users WHERE id = ?1", [user_id]).map(|_| ()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn users_credits_and_items() {
        let db = Db::open(":memory:").unwrap();
        let id = db.create_user("a@b.co", "hash").unwrap().unwrap();
        assert_eq!(db.create_user("a@b.co", "x").unwrap(), None);
        assert_eq!(db.user(id).unwrap().unwrap().plan, Plan::Free);
        assert!(db.reserve_credits(id, 2, 3).unwrap());
        assert!(!db.reserve_credits(id, 2, 3).unwrap());
        assert!(db.reserve_credits(id, 1, 3).unwrap());
        assert_eq!(db.credits_used(id).unwrap(), 3);
        db.refund_credits(id, 1).unwrap();
        assert_eq!(db.credits_used(id).unwrap(), 2);
        db.insert(Kind::Report, "r1", id, "Bracket", "{}", Some(80), None).unwrap();
        db.insert(Kind::Project, "p1", id, "Job", "{\"a\":1}", None, None).unwrap();
        assert_eq!(db.storage_used(id).unwrap(), (2 + 7 + 7 + 3) as u64);
        let l = db.list(Kind::Report, id).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].score, Some(80));
        assert_eq!(db.get(Kind::Report, id, "r1").unwrap().as_deref(), Some("{}"));
        assert_eq!(db.get(Kind::Report, id + 1, "r1").unwrap(), None);
        assert!(db.delete(Kind::Report, id, "r1").unwrap());
        assert!(!db.delete(Kind::Report, id, "r1").unwrap());
        db.create_session("h", id, 60).unwrap();
        assert_eq!(db.session_user("h").unwrap(), Some(id));
        db.set_plan(id, Plan::Pro, Some("cus_1")).unwrap();
        assert_eq!(db.set_plan_by_customer("cus_1", Plan::Free).unwrap(), 1);
        db.delete_user(id).unwrap();
        assert_eq!(db.session_user("h").unwrap(), None);
    }
}
