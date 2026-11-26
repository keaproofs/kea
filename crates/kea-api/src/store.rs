//! The store: reports by address, and the job queue, in one SQLite file.

use anyhow::{Context, Result};
use kea_core::ContractRun;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS reports (
    address TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    implementation TEXT,
    code_hash TEXT NOT NULL,
    run TEXT NOT NULL,
    test_source TEXT NOT NULL,
    review TEXT NOT NULL,
    question TEXT,
    cost_usd REAL NOT NULL,
    checked_at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS reports_by_code ON reports(code_hash);
CREATE INDEX IF NOT EXISTS reports_by_time ON reports(checked_at);
CREATE TABLE IF NOT EXISTS jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    address TEXT NOT NULL,
    question TEXT,
    state TEXT NOT NULL,
    error TEXT,
    requester TEXT NOT NULL,
    code_hash TEXT NOT NULL,
    implementation TEXT,
    cost_usd REAL NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    started_at INTEGER,
    finished_at INTEGER
);
CREATE INDEX IF NOT EXISTS jobs_by_state ON jobs(state, id);
CREATE INDEX IF NOT EXISTS jobs_by_requester ON jobs(requester, created_at);
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    address TEXT NOT NULL,
    kind TEXT NOT NULL,
    detail TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS events_by_address ON events(address, at);
CREATE TABLE IF NOT EXISTS stale (
    address TEXT PRIMARY KEY,
    since INTEGER NOT NULL,
    code_hash TEXT NOT NULL
);
";

/// Seconds since the Unix epoch.
pub fn now() -> i64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| i64::try_from(since.as_secs()).unwrap_or(i64::MAX))
}

/// The start of the UTC day that holds `at`.
pub fn day_start(at: i64) -> i64 {
    at - at.rem_euclid(86_400)
}

/// A proved contract, as the engine left it.
#[derive(Clone, Debug)]
pub struct Report {
    /// Lower-case `0x` address.
    pub address: String,
    /// The contract name without its source path.
    pub name: String,
    /// The implementation a proxy points at.
    pub implementation: Option<String>,
    /// Keccak of the runtime code that runs (the implementation's, for a proxy).
    pub code_hash: String,
    /// The kept attempt.
    pub run: ContractRun,
    /// Its test file.
    pub test_source: String,
    /// Its review entries.
    pub review: BTreeMap<String, Value>,
    /// The question that shaped it, if any. Stored, never published.
    pub question: Option<String>,
    /// What drafting and review cost.
    pub cost_usd: f64,
    /// When it finished.
    pub checked_at: i64,
}

/// Where a job is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobState {
    /// Waiting for the worker.
    Queued,
    /// Being proved.
    Running,
    /// Finished with a report.
    Done,
    /// Finished without one.
    Failed,
}

impl JobState {
    fn name(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Done => "done",
            Self::Failed => "failed",
        }
    }

    fn parse(name: &str) -> Self {
        match name {
            "queued" => Self::Queued,
            "running" => Self::Running,
            "done" => Self::Done,
            _ => Self::Failed,
        }
    }

    /// The name the API shows.
    pub fn as_str(self) -> &'static str {
        self.name()
    }
}

/// A request to prove one contract.
#[derive(Clone, Debug)]
pub struct Job {
    /// Its number.
    pub id: i64,
    /// Lower-case `0x` address.
    pub address: String,
    /// A person's question, answered first.
    pub question: Option<String>,
    /// Where it is.
    pub state: JobState,
    /// Why it failed.
    pub error: Option<String>,
    /// Keccak of the code that ran when it was asked.
    pub code_hash: String,
    /// The implementation a proxy pointed at.
    pub implementation: Option<String>,
    /// When it was asked.
    pub created_at: i64,
    /// When the worker took it.
    pub started_at: Option<i64>,
    /// When it finished.
    pub finished_at: Option<i64>,
}

/// What a new job needs.
pub struct NewJob<'a> {
    /// Lower-case `0x` address.
    pub address: &'a str,
    /// A person's question.
    pub question: Option<&'a str>,
    /// A salted hash of who asked, for the daily limit.
    pub requester: &'a str,
    /// Keccak of the code that runs.
    pub code_hash: &'a str,
    /// The implementation a proxy points at.
    pub implementation: Option<&'a str>,
}

/// Something that happened to a watched contract after it was proved.
#[derive(Clone, Debug)]
pub struct Event {
    /// Its number, rising with time.
    pub id: i64,
    /// Lower-case `0x` address.
    pub address: String,
    /// What happened, such as `code-changed`.
    pub kind: String,
    /// The facts of it.
    pub detail: Value,
    /// When it was seen.
    pub at: i64,
}

/// The SQLite store. Calls are short; one connection behind a lock serves them.
// Known limit: one connection behind one lock; a pool or spawn_blocking once traffic makes the lock show up.
#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    /// Opens (or creates) the store at `path`.
    ///
    /// # Errors
    /// When the file cannot be opened or the schema cannot be applied.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        let conn = Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::ready(conn)
    }

    /// A store in memory, for tests.
    #[cfg(test)]
    pub fn in_memory() -> Result<Self> {
        Self::ready(Connection::open_in_memory()?)
    }

    fn ready(conn: Connection) -> Result<Self> {
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    fn with<T>(&self, f: impl FnOnce(&Connection) -> rusqlite::Result<T>) -> Result<T> {
        let conn = self.conn.lock().map_err(|_| anyhow::anyhow!("the store lock was poisoned"))?;
        Ok(f(&conn)?)
    }

    /// Queues a job and returns its number.
    ///
    /// # Errors
    /// When the store fails.
    pub fn enqueue(&self, job: &NewJob<'_>) -> Result<i64> {
        self.with(|conn| {
            conn.execute(
                "INSERT INTO jobs (address, question, state, requester, code_hash, implementation, created_at)
                 VALUES (?1, ?2, 'queued', ?3, ?4, ?5, ?6)",
                params![job.address, job.question, job.requester, job.code_hash, job.implementation, now()],
            )?;
            Ok(conn.last_insert_rowid())
        })
    }

    /// One job.
    ///
    /// # Errors
    /// When the store fails.
    pub fn job(&self, id: i64) -> Result<Option<Job>> {
        self.with(|conn| conn.query_row(&format!("{JOB_COLUMNS} WHERE id = ?1"), [id], job_from_row).optional())
    }

    /// How many jobs wait ahead of `id`.
    ///
    /// # Errors
    /// When the store fails.
    pub fn position(&self, id: i64) -> Result<usize> {
        self.count("SELECT COUNT(*) FROM jobs WHERE state IN ('queued', 'running') AND id < ?1", [id])
    }

    /// Takes the oldest queued job and marks it running.
    ///
    /// # Errors
    /// When the store fails.
    pub fn claim_next(&self) -> Result<Option<Job>> {
        let id: Option<i64> = self.with(|conn| {
            conn.query_row("SELECT id FROM jobs WHERE state = 'queued' ORDER BY id LIMIT 1", [], |row| row.get(0))
                .optional()
        })?;
        let Some(id) = id else { return Ok(None) };
        self.with(|conn| {
            conn.execute("UPDATE jobs SET state = 'running', started_at = ?2 WHERE id = ?1", params![id, now()])
        })?;
        self.job(id)
    }

    /// Ends a job: done, or failed with why. Either way with what it spent, since a failed job can
    /// have paid for drafts.
    ///
    /// # Errors
    /// When the store fails.
    pub fn finish(&self, id: i64, outcome: std::result::Result<(), &str>, cost: f64) -> Result<()> {
        let (state, error) = match outcome {
            Ok(()) => (JobState::Done, None),
            Err(error) => (JobState::Failed, Some(error)),
        };
        self.with(|conn| {
            conn.execute(
                "UPDATE jobs SET state = ?2, error = ?3, cost_usd = ?4, finished_at = ?5 WHERE id = ?1",
                params![id, state.name(), error, cost, now()],
            )
        })?;
        Ok(())
    }

    /// Puts jobs a restart interrupted back in the queue.
    ///
    /// # Errors
    /// When the store fails.
    pub fn requeue_interrupted(&self) -> Result<usize> {
        self.with(|conn| {
            conn.execute("UPDATE jobs SET state = 'queued', started_at = NULL WHERE state = 'running'", [])
        })
    }

    /// The open job for an address, so two people asking at once share one run.
    ///
    /// # Errors
    /// When the store fails.
    pub fn open_job_for(&self, address: &str) -> Result<Option<i64>> {
        self.with(|conn| {
            conn.query_row(
                "SELECT id FROM jobs WHERE address = ?1 AND state IN ('queued', 'running') ORDER BY id LIMIT 1",
                [address],
                |row| row.get(0),
            )
            .optional()
        })
    }

    /// When anyone last asked for a check of `address`, the watcher included.
    ///
    /// # Errors
    /// When the store fails.
    pub fn last_asked(&self, address: &str) -> Result<Option<i64>> {
        self.with(|conn| {
            conn.query_row("SELECT MAX(created_at) FROM jobs WHERE address = ?1", [address], |row| row.get(0))
        })
    }

    /// Jobs waiting or running.
    ///
    /// # Errors
    /// When the store fails.
    pub fn open_jobs(&self) -> Result<usize> {
        self.count("SELECT COUNT(*) FROM jobs WHERE state IN ('queued', 'running')", [])
    }

    /// Jobs one requester asked for since `since`.
    ///
    /// # Errors
    /// When the store fails.
    pub fn asks_since(&self, requester: &str, since: i64) -> Result<usize> {
        self.count("SELECT COUNT(*) FROM jobs WHERE requester = ?1 AND created_at >= ?2", params![requester, since])
    }

    /// US dollars finished jobs spent since `since`.
    ///
    /// # Errors
    /// When the store fails.
    pub fn spent_since(&self, since: i64) -> Result<f64> {
        self.with(|conn| {
            conn.query_row("SELECT COALESCE(SUM(cost_usd), 0) FROM jobs WHERE finished_at >= ?1", [since], |row| {
                row.get(0)
            })
        })
    }

    fn count(&self, sql: &str, params: impl rusqlite::Params) -> Result<usize> {
        let count: i64 = self.with(|conn| conn.query_row(sql, params, |row| row.get(0)))?;
        Ok(usize::try_from(count).unwrap_or(0))
    }

    /// Saves a report, replacing the address's earlier one.
    ///
    /// # Errors
    /// When the store fails.
    pub fn save_report(&self, report: &Report) -> Result<()> {
        let run = serde_json::to_string(&report.run)?;
        let review = serde_json::to_string(&report.review)?;
        self.with(|conn| {
            conn.execute(
                "INSERT OR REPLACE INTO reports
                 (address, name, implementation, code_hash, run, test_source, review, question, cost_usd, checked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    report.address,
                    report.name,
                    report.implementation,
                    report.code_hash,
                    run,
                    report.test_source,
                    review,
                    report.question,
                    report.cost_usd,
                    report.checked_at
                ],
            )
        })?;
        // A fresh report answers whatever made the old one stale.
        self.with(|conn| conn.execute("DELETE FROM stale WHERE address = ?1", [&report.address]))?;
        Ok(())
    }

    /// Every proved address with the code hash its report was proved on.
    ///
    /// # Errors
    /// When the store fails.
    pub fn watched(&self) -> Result<Vec<(String, String)>> {
        self.with(|conn| {
            let mut statement = conn.prepare("SELECT address, code_hash FROM reports ORDER BY address")?;
            statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.collect()
        })
    }

    /// Marks a report stale because the code at its address is now `code_hash`. False when that
    /// change was already recorded, so one change raises one event.
    ///
    /// # Errors
    /// When the store fails.
    pub fn mark_stale(&self, address: &str, code_hash: &str) -> Result<bool> {
        let changed = self.with(|conn| {
            conn.execute(
                "INSERT INTO stale (address, since, code_hash) VALUES (?1, ?2, ?3)
                 ON CONFLICT(address) DO UPDATE SET since = excluded.since, code_hash = excluded.code_hash
                 WHERE stale.code_hash != excluded.code_hash",
                params![address, now(), code_hash],
            )
        })?;
        Ok(changed > 0)
    }

    /// Since when an address's report no longer matches its code.
    ///
    /// # Errors
    /// When the store fails.
    pub fn stale_since(&self, address: &str) -> Result<Option<i64>> {
        self.with(|conn| {
            conn.query_row("SELECT since FROM stale WHERE address = ?1", [address], |row| row.get(0)).optional()
        })
    }

    /// Records an event.
    ///
    /// # Errors
    /// When the store fails.
    pub fn add_event(&self, address: &str, kind: &str, detail: &Value) -> Result<()> {
        let detail = serde_json::to_string(detail)?;
        self.with(|conn| {
            conn.execute(
                "INSERT INTO events (address, kind, detail, at) VALUES (?1, ?2, ?3, ?4)",
                params![address, kind, detail, now()],
            )
        })?;
        Ok(())
    }

    /// The newest events, for one address or for all.
    ///
    /// # Errors
    /// When the store fails.
    pub fn events(&self, address: Option<&str>, limit: usize) -> Result<Vec<Event>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        self.with(|conn| {
            let mut statement = conn.prepare(
                "SELECT id, address, kind, detail, at FROM events WHERE ?1 IS NULL OR address = ?1
                 ORDER BY id DESC LIMIT ?2",
            )?;
            statement
                .query_map(params![address, limit], |row| {
                    let detail: String = row.get(3)?;
                    Ok(Event {
                        id: row.get(0)?,
                        address: row.get(1)?,
                        kind: row.get(2)?,
                        detail: serde_json::from_str(&detail).unwrap_or(Value::Null),
                        at: row.get(4)?,
                    })
                })?
                .collect()
        })
    }

    /// The report for an address.
    ///
    /// # Errors
    /// When the store fails or a stored report no longer parses.
    pub fn report(&self, address: &str) -> Result<Option<Report>> {
        self.report_where("address = ?1", address)
    }

    /// The newest report for other code identical to `code_hash`: proved once, read by every twin.
    ///
    /// # Errors
    /// When the store fails or a stored report no longer parses.
    pub fn twin(&self, code_hash: &str, not_address: &str) -> Result<Option<Report>> {
        let row = self.with(|conn| {
            conn.query_row(
                &format!("{REPORT_COLUMNS} WHERE code_hash = ?1 AND address != ?2 ORDER BY checked_at DESC LIMIT 1"),
                params![code_hash, not_address],
                raw_report,
            )
            .optional()
        })?;
        row.map(RawReport::parse).transpose()
    }

    fn report_where(&self, filter: &str, value: &str) -> Result<Option<Report>> {
        let row = self
            .with(|conn| conn.query_row(&format!("{REPORT_COLUMNS} WHERE {filter}"), [value], raw_report).optional())?;
        row.map(RawReport::parse).transpose()
    }

    /// The newest reports, newest first.
    ///
    /// # Errors
    /// When the store fails.
    pub fn recent(&self, limit: usize) -> Result<Vec<Report>> {
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        let rows = self.with(|conn| {
            let mut statement = conn.prepare(&format!("{REPORT_COLUMNS} ORDER BY checked_at DESC LIMIT ?1"))?;
            statement.query_map([limit], raw_report)?.collect::<rusqlite::Result<Vec<RawReport>>>()
        })?;
        rows.into_iter().map(RawReport::parse).collect()
    }
}

const JOB_COLUMNS: &str = "SELECT id, address, question, state, error, code_hash, implementation, created_at, \
                           started_at, finished_at FROM jobs";
const REPORT_COLUMNS: &str = "SELECT address, name, implementation, code_hash, run, test_source, review, question, \
                              cost_usd, checked_at FROM reports";

fn job_from_row(row: &Row<'_>) -> rusqlite::Result<Job> {
    Ok(Job {
        id: row.get(0)?,
        address: row.get(1)?,
        question: row.get(2)?,
        state: JobState::parse(&row.get::<_, String>(3)?),
        error: row.get(4)?,
        code_hash: row.get(5)?,
        implementation: row.get(6)?,
        created_at: row.get(7)?,
        started_at: row.get(8)?,
        finished_at: row.get(9)?,
    })
}

/// A report row before its JSON columns are parsed.
struct RawReport {
    address: String,
    name: String,
    implementation: Option<String>,
    code_hash: String,
    run: String,
    test_source: String,
    review: String,
    question: Option<String>,
    cost_usd: f64,
    checked_at: i64,
}

fn raw_report(row: &Row<'_>) -> rusqlite::Result<RawReport> {
    Ok(RawReport {
        address: row.get(0)?,
        name: row.get(1)?,
        implementation: row.get(2)?,
        code_hash: row.get(3)?,
        run: row.get(4)?,
        test_source: row.get(5)?,
        review: row.get(6)?,
        question: row.get(7)?,
        cost_usd: row.get(8)?,
        checked_at: row.get(9)?,
    })
}

impl RawReport {
    fn parse(self) -> Result<Report> {
        Ok(Report {
            run: serde_json::from_str(&self.run).with_context(|| format!("stored run of {}", self.address))?,
            review: serde_json::from_str(&self.review).with_context(|| format!("stored review of {}", self.address))?,
            address: self.address,
            name: self.name,
            implementation: self.implementation,
            code_hash: self.code_hash,
            test_source: self.test_source,
            question: self.question,
            cost_usd: self.cost_usd,
            checked_at: self.checked_at,
        })
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn report(address: &str, code_hash: &str, checked_at: i64) -> Report {
        Report {
            address: address.into(),
            name: "PriceFeed".into(),
            implementation: None,
            code_hash: code_hash.into(),
            run: serde_json::from_value(
                serde_json::json!({"address": address, "name": "c.sol:PriceFeed", "claims": []}),
            )
            .unwrap(),
            test_source: String::new(),
            review: BTreeMap::new(),
            question: None,
            cost_usd: 0.3,
            checked_at,
        }
    }

    fn job<'a>(address: &'a str, requester: &'a str) -> NewJob<'a> {
        NewJob { address, question: None, requester, code_hash: "0xc0de", implementation: None }
    }

    #[test]
    fn the_queue_runs_oldest_first_and_counts_positions() {
        let store = Store::in_memory().unwrap();
        let first = store.enqueue(&job("0xa", "r1")).unwrap();
        let second = store.enqueue(&job("0xb", "r1")).unwrap();
        assert_eq!(store.position(first).unwrap(), 0);
        assert_eq!(store.position(second).unwrap(), 1);
        assert_eq!(store.open_job_for("0xb").unwrap(), Some(second));
        let taken = store.claim_next().unwrap().unwrap();
        assert_eq!((taken.id, taken.state), (first, JobState::Running));
        assert_eq!(store.requeue_interrupted().unwrap(), 1, "a restart puts the running job back");
        assert_eq!(store.claim_next().unwrap().unwrap().id, first);
        store.finish(first, Ok(()), 0.25).unwrap();
        store.claim_next().unwrap();
        store.finish(second, Err("the drafter stopped answering"), 0.1).unwrap();
        assert_eq!(store.job(second).unwrap().unwrap().error.as_deref(), Some("the drafter stopped answering"));
        assert_eq!(store.open_jobs().unwrap(), 0);
        assert!((store.spent_since(0).unwrap() - 0.35).abs() < 1e-9, "a failed job's drafts count too");
        assert!(store.last_asked("0xff").unwrap().is_none());
        assert!(store.last_asked("0xa").unwrap().is_some());
        assert_eq!(store.asks_since("r1", 0).unwrap(), 2);
        assert_eq!(store.asks_since("r2", 0).unwrap(), 0);
    }

    #[test]
    fn reports_replace_by_address_and_twins_share_code() {
        let store = Store::in_memory().unwrap();
        store.save_report(&report("0xa", "0xc0de", 10)).unwrap();
        store.save_report(&report("0xb", "0xc0de", 20)).unwrap();
        store.save_report(&report("0xa", "0xc0de", 30)).unwrap();
        assert_eq!(store.report("0xa").unwrap().unwrap().checked_at, 30);
        assert_eq!(store.twin("0xc0de", "0xc").unwrap().unwrap().address, "0xa", "the newest twin");
        assert_eq!(store.twin("0xc0de", "0xa").unwrap().unwrap().address, "0xb");
        assert!(store.twin("0xbeef", "0xa").unwrap().is_none());
        let recent: Vec<String> = store.recent(5).unwrap().into_iter().map(|r| r.address).collect();
        assert_eq!(recent, ["0xa", "0xb"]);
    }

    #[test]
    fn a_code_change_is_recorded_once_and_a_fresh_report_clears_it() {
        let store = Store::in_memory().unwrap();
        store.save_report(&report("0xa", "0xc0de", 1)).unwrap();
        assert_eq!(store.watched().unwrap(), [("0xa".to_string(), "0xc0de".to_string())]);
        assert!(store.mark_stale("0xa", "0xbeef").unwrap());
        assert!(!store.mark_stale("0xa", "0xbeef").unwrap(), "the same change twice is one event");
        assert!(store.mark_stale("0xa", "0xf00d").unwrap(), "a second upgrade is news");
        assert!(store.stale_since("0xa").unwrap().is_some());
        store.add_event("0xa", "code-changed", &serde_json::json!({"to": "0xf00d"})).unwrap();
        store.add_event("0xb", "code-changed", &serde_json::json!({})).unwrap();
        assert_eq!(store.events(Some("0xa"), 10).unwrap().len(), 1);
        assert_eq!(store.events(None, 10).unwrap()[0].address, "0xb", "newest first");
        store.save_report(&report("0xa", "0xf00d", 2)).unwrap();
        assert_eq!(store.stale_since("0xa").unwrap(), None);
    }

    #[test]
    fn days_start_at_midnight_utc() {
        assert_eq!(day_start(86_400 * 3 + 5), 86_400 * 3);
        assert_eq!(day_start(86_400 * 3), 86_400 * 3);
    }
}
