//! SQLite persistence for jobs.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::from_str;

use crate::job::{BackendKind, Job, JobErrorInfo, JobId, JobOptions, JobRecord, JobState, Source};
use crate::util::now_millis;

const SCHEMA_MIGRATIONS: &str = "
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
);
";

const MIGRATION_1: &str = "
CREATE TABLE jobs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    source_json TEXT NOT NULL,
    filename TEXT NOT NULL,
    target_dir TEXT NOT NULL,
    state TEXT NOT NULL,
    backend TEXT NOT NULL,
    total_bytes INTEGER,
    downloaded_bytes INTEGER NOT NULL DEFAULT 0,
    supports_ranges INTEGER NOT NULL DEFAULT 0,
    etag TEXT,
    last_modified TEXT,
    content_type TEXT,
    error_message TEXT,
    error_retryable INTEGER NOT NULL DEFAULT 0,
    options_json TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL
);
CREATE INDEX idx_jobs_state ON jobs(state);
";

const MIGRATIONS: &[(i64, &str)] = &[(1, MIGRATION_1)];

const JOB_COLUMNS: &str = "
id, source_json, filename, target_dir, state, backend, total_bytes, downloaded_bytes,
supports_ranges, etag, last_modified, content_type, error_message, error_retryable,
options_json, created_at, updated_at
";

/// A SQLite-backed job repository.
pub struct Store {
    connection: Mutex<Connection>,
}

impl Store {
    /// Opens a database file, creating parent directories and applying migrations.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let path = path.as_ref();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }

        let connection = Connection::open(path)?;
        Self::from_connection(connection)
    }

    /// Opens an in-memory database and applies migrations.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        let connection = Connection::open_in_memory()?;
        Self::from_connection(connection)
    }

    /// Returns the highest applied migration version.
    pub fn schema_version(&self) -> Result<i64, StoreError> {
        let connection = self.lock()?;
        let version = connection.query_row(
            "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
            [],
            |row| row.get(0),
        )?;
        Ok(version)
    }

    /// Inserts a job and assigns its generated id.
    pub fn insert_job(&self, job: &mut Job) -> Result<(), StoreError> {
        let record = job.record();
        let source_json = serde_json::to_string(&record.source)?;
        let options_json = serde_json::to_string(&record.options)?;
        let (error_message, error_retryable) = error_columns(record.error.as_ref());

        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO jobs (
                source_json, filename, target_dir, state, backend, total_bytes,
                downloaded_bytes, supports_ranges, etag, last_modified, content_type,
                error_message, error_retryable, options_json, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                source_json,
                record.filename,
                record.target_dir.to_string_lossy(),
                state_name(record.state),
                backend_name(record.backend),
                record.total_bytes.map(|bytes| bytes as i64),
                record.downloaded_bytes as i64,
                if record.supports_ranges { 1 } else { 0 },
                record.etag,
                record.last_modified,
                record.content_type,
                error_message,
                error_retryable,
                options_json,
                record.created_at,
                record.updated_at,
            ],
        )?;

        job.set_id(JobId(connection.last_insert_rowid()));
        Ok(())
    }

    /// Loads one job by id.
    pub fn get_job(&self, id: JobId) -> Result<Job, StoreError> {
        let connection = self.lock()?;
        let mut statement =
            connection.prepare(&format!("SELECT {JOB_COLUMNS} FROM jobs WHERE id = ?1"))?;
        let row = statement
            .query_row(params![id.0], db_job_from_row)
            .optional()?;

        match row {
            Some(row) => to_job(row),
            None => Err(StoreError::JobNotFound(id)),
        }
    }

    /// Persists every mutable field of an existing job.
    pub fn update_job(&self, job: &Job) -> Result<(), StoreError> {
        let record = job.record();
        let source_json = serde_json::to_string(&record.source)?;
        let options_json = serde_json::to_string(&record.options)?;
        let (error_message, error_retryable) = error_columns(record.error.as_ref());

        let connection = self.lock()?;
        let changed = connection.execute(
            "UPDATE jobs SET
                source_json = ?2, filename = ?3, target_dir = ?4, state = ?5, backend = ?6,
                total_bytes = ?7, downloaded_bytes = ?8, supports_ranges = ?9, etag = ?10,
                last_modified = ?11, content_type = ?12, error_message = ?13,
                error_retryable = ?14, options_json = ?15, created_at = ?16, updated_at = ?17
            WHERE id = ?1",
            params![
                record.id.0,
                source_json,
                record.filename,
                record.target_dir.to_string_lossy(),
                state_name(record.state),
                backend_name(record.backend),
                record.total_bytes.map(|bytes| bytes as i64),
                record.downloaded_bytes as i64,
                if record.supports_ranges { 1 } else { 0 },
                record.etag,
                record.last_modified,
                record.content_type,
                error_message,
                error_retryable,
                options_json,
                record.created_at,
                record.updated_at,
            ],
        )?;

        if changed == 0 {
            return Err(StoreError::JobNotFound(record.id));
        }
        Ok(())
    }

    /// Lists every job ordered by id.
    pub fn list_jobs(&self) -> Result<Vec<Job>, StoreError> {
        let connection = self.lock()?;
        let mut statement =
            connection.prepare(&format!("SELECT {JOB_COLUMNS} FROM jobs ORDER BY id ASC"))?;
        let rows = statement.query_map([], db_job_from_row)?;

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(to_job(row?)?);
        }
        Ok(jobs)
    }

    fn from_connection(connection: Connection) -> Result<Self, StoreError> {
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.pragma_update(None, "busy_timeout", 5000_i64)?;

        let store = Self {
            connection: Mutex::new(connection),
        };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        transaction.execute_batch(SCHEMA_MIGRATIONS)?;

        let current: Option<i64> =
            transaction.query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })?;
        let current = current.unwrap_or(0);

        for &(version, sql) in MIGRATIONS {
            if version > current {
                transaction.execute_batch(sql)?;
                transaction.execute(
                    "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
                    params![version, now_millis()],
                )?;
            }
        }

        transaction.commit()?;
        Ok(())
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, StoreError> {
        self.connection.lock().map_err(|_| StoreError::LockPoisoned)
    }
}

/// Errors returned by the job store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),
    #[error("job not found: {0:?}")]
    JobNotFound(JobId),
    #[error("store lock poisoned")]
    LockPoisoned,
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid job state: {0}")]
    InvalidState(String),
    #[error("invalid backend: {0}")]
    InvalidBackend(String),
}

struct DbJob {
    id: i64,
    source_json: String,
    filename: String,
    target_dir: String,
    state: String,
    backend: String,
    total_bytes: Option<i64>,
    downloaded_bytes: i64,
    supports_ranges: i64,
    etag: Option<String>,
    last_modified: Option<String>,
    content_type: Option<String>,
    error_message: Option<String>,
    error_retryable: i64,
    options_json: String,
    created_at: i64,
    updated_at: i64,
}

fn db_job_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<DbJob> {
    Ok(DbJob {
        id: row.get(0)?,
        source_json: row.get(1)?,
        filename: row.get(2)?,
        target_dir: row.get(3)?,
        state: row.get(4)?,
        backend: row.get(5)?,
        total_bytes: row.get(6)?,
        downloaded_bytes: row.get(7)?,
        supports_ranges: row.get(8)?,
        etag: row.get(9)?,
        last_modified: row.get(10)?,
        content_type: row.get(11)?,
        error_message: row.get(12)?,
        error_retryable: row.get(13)?,
        options_json: row.get(14)?,
        created_at: row.get(15)?,
        updated_at: row.get(16)?,
    })
}

fn to_job(row: DbJob) -> Result<Job, StoreError> {
    let source: Source = from_str(&row.source_json)?;
    let options: JobOptions = from_str(&row.options_json)?;
    let state = parse_state(&row.state)?;
    let backend = parse_backend(&row.backend)?;
    let error = row.error_message.map(|message| JobErrorInfo {
        message,
        retryable: row.error_retryable != 0,
    });

    Ok(Job::from_record(JobRecord {
        id: JobId(row.id),
        source,
        filename: row.filename,
        target_dir: PathBuf::from(row.target_dir),
        state,
        backend,
        total_bytes: row.total_bytes.map(|bytes| bytes as u64),
        downloaded_bytes: row.downloaded_bytes as u64,
        supports_ranges: row.supports_ranges != 0,
        etag: row.etag,
        last_modified: row.last_modified,
        content_type: row.content_type,
        error,
        options,
        created_at: row.created_at,
        updated_at: row.updated_at,
    }))
}

fn error_columns(error: Option<&JobErrorInfo>) -> (Option<&str>, i64) {
    match error {
        Some(error) => (
            Some(error.message.as_str()),
            if error.retryable { 1 } else { 0 },
        ),
        None => (None, 0),
    }
}

fn state_name(state: JobState) -> &'static str {
    match state {
        JobState::Queued => "queued",
        JobState::Probing => "probing",
        JobState::Downloading => "downloading",
        JobState::Paused => "paused",
        JobState::Verifying => "verifying",
        JobState::Seeding => "seeding",
        JobState::Completed => "completed",
        JobState::Failed => "failed",
        JobState::Cancelled => "cancelled",
    }
}

fn parse_state(name: &str) -> Result<JobState, StoreError> {
    match name {
        "queued" => Ok(JobState::Queued),
        "probing" => Ok(JobState::Probing),
        "downloading" => Ok(JobState::Downloading),
        "paused" => Ok(JobState::Paused),
        "verifying" => Ok(JobState::Verifying),
        "seeding" => Ok(JobState::Seeding),
        "completed" => Ok(JobState::Completed),
        "failed" => Ok(JobState::Failed),
        "cancelled" => Ok(JobState::Cancelled),
        other => Err(StoreError::InvalidState(other.to_string())),
    }
}

fn backend_name(backend: BackendKind) -> &'static str {
    match backend {
        BackendKind::Http => "http",
        BackendKind::Torrent => "torrent",
        BackendKind::Hls => "hls",
        BackendKind::Dash => "dash",
        BackendKind::YtDlp => "yt_dlp",
        BackendKind::Plugin => "plugin",
    }
}

fn parse_backend(name: &str) -> Result<BackendKind, StoreError> {
    match name {
        "http" => Ok(BackendKind::Http),
        "torrent" => Ok(BackendKind::Torrent),
        "hls" => Ok(BackendKind::Hls),
        "dash" => Ok(BackendKind::Dash),
        "yt_dlp" => Ok(BackendKind::YtDlp),
        "plugin" => Ok(BackendKind::Plugin),
        other => Err(StoreError::InvalidBackend(other.to_string())),
    }
}
