//! SQLite persistence for jobs.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::from_str;

use crate::job::{BackendKind, Job, JobErrorInfo, JobId, JobOptions, JobRecord, JobState, Source};
use crate::queue::Queue;
use crate::segment::Segment;
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

const MIGRATION_3: &str = "
CREATE TABLE IF NOT EXISTS settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
";

const MIGRATIONS: &[(i64, &str)] = &[(1, MIGRATION_1), (2, MIGRATION_2), (3, MIGRATION_3)];

const MIGRATION_2: &str = "
CREATE TABLE queues (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL UNIQUE,
    max_concurrent INTEGER NOT NULL DEFAULT 3
);
INSERT INTO queues (id, name, max_concurrent) VALUES (1, 'default', 3);
ALTER TABLE jobs ADD COLUMN queue_id INTEGER NOT NULL DEFAULT 1;
ALTER TABLE jobs ADD COLUMN position INTEGER NOT NULL DEFAULT 0;
CREATE INDEX idx_jobs_queue ON jobs(queue_id, position);
CREATE TABLE segments (
    job_id INTEGER NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    idx INTEGER NOT NULL,
    start INTEGER NOT NULL,
    \"end\" INTEGER NOT NULL,
    downloaded INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (job_id, idx)
);
";

const JOB_COLUMNS: &str = "
id, source_json, filename, target_dir, state, queue_id, position, backend, total_bytes,
downloaded_bytes, supports_ranges, etag, last_modified, content_type, error_message,
error_retryable, options_json, created_at, updated_at
";

/// A SQLite-backed job repository.
#[derive(Clone)]
pub struct Store {
    connection: Arc<Mutex<Connection>>,
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

    /// Inserts a job and assigns its generated id and queue position.
    pub fn insert_job(&self, job: &mut Job) -> Result<(), StoreError> {
        let record = job.record();
        let source_json = serde_json::to_string(&record.source)?;
        let options_json = serde_json::to_string(&record.options)?;
        let (error_message, error_retryable) = error_columns(record.error.as_ref());

        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        let position: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(position), 0) + 1 FROM jobs WHERE queue_id = ?1",
            params![record.queue_id],
            |row| row.get(0),
        )?;
        transaction.execute(
            "INSERT INTO jobs (
                source_json, filename, target_dir, state, queue_id, position, backend,
                total_bytes, downloaded_bytes, supports_ranges, etag, last_modified, content_type,
                error_message, error_retryable, options_json, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18)",
            params![
                source_json,
                record.filename,
                record.target_dir.to_string_lossy(),
                state_name(record.state),
                record.queue_id,
                position,
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

        let id = transaction.last_insert_rowid();
        transaction.commit()?;

        job.set_id(JobId(id));
        job.set_queue_position(record.queue_id, position);
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
                source_json = ?2, filename = ?3, target_dir = ?4, state = ?5, queue_id = ?6,
                position = ?7, backend = ?8, total_bytes = ?9, downloaded_bytes = ?10,
                supports_ranges = ?11, etag = ?12, last_modified = ?13, content_type = ?14,
                error_message = ?15, error_retryable = ?16, options_json = ?17,
                created_at = ?18, updated_at = ?19
            WHERE id = ?1",
            params![
                record.id.0,
                source_json,
                record.filename,
                record.target_dir.to_string_lossy(),
                state_name(record.state),
                record.queue_id,
                record.position,
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

    /// Replaces all persisted segments of a job.
    pub fn replace_segments(&self, job: JobId, segments: &[Segment]) -> Result<(), StoreError> {
        let mut connection = self.lock()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM segments WHERE job_id = ?1", params![job.0])?;
        {
            let mut statement = transaction.prepare(
                "INSERT INTO segments (job_id, idx, start, \"end\", downloaded)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
            )?;
            for segment in segments {
                statement.execute(params![
                    job.0,
                    i64::from(segment.index),
                    segment.start as i64,
                    segment.end as i64,
                    segment.downloaded as i64,
                ])?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    /// Lists the segments of a job ordered by index.
    pub fn list_segments(&self, job: JobId) -> Result<Vec<Segment>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(
            "SELECT idx, start, \"end\", downloaded FROM segments
             WHERE job_id = ?1 ORDER BY idx ASC",
        )?;
        let rows = statement.query_map(params![job.0], |row| {
            Ok(Segment {
                index: row.get::<_, i64>(0)? as u32,
                start: row.get::<_, i64>(1)? as u64,
                end: row.get::<_, i64>(2)? as u64,
                downloaded: row.get::<_, i64>(3)? as u64,
            })
        })?;

        let mut segments = Vec::new();
        for row in rows {
            segments.push(row?);
        }
        Ok(segments)
    }

    /// Deletes all persisted segments of a job.
    pub fn clear_segments(&self, job: JobId) -> Result<(), StoreError> {
        let connection = self.lock()?;
        connection.execute("DELETE FROM segments WHERE job_id = ?1", params![job.0])?;
        Ok(())
    }

    /// Creates a named queue.
    pub fn create_queue(&self, name: &str, max_concurrent: u32) -> Result<Queue, StoreError> {
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO queues (name, max_concurrent) VALUES (?1, ?2)",
            params![name, i64::from(max_concurrent)],
        )?;
        Ok(Queue {
            id: connection.last_insert_rowid(),
            name: name.to_string(),
            max_concurrent,
        })
    }

    /// Lists every queue ordered by id.
    pub fn list_queues(&self) -> Result<Vec<Queue>, StoreError> {
        let connection = self.lock()?;
        let mut statement =
            connection.prepare("SELECT id, name, max_concurrent FROM queues ORDER BY id ASC")?;
        let rows = statement.query_map([], |row| {
            Ok(Queue {
                id: row.get(0)?,
                name: row.get(1)?,
                max_concurrent: row.get::<_, i64>(2)? as u32,
            })
        })?;

        let mut queues = Vec::new();
        for row in rows {
            queues.push(row?);
        }
        Ok(queues)
    }

    /// Loads one queue by name.
    pub fn queue_by_name(&self, name: &str) -> Result<Queue, StoreError> {
        let connection = self.lock()?;
        let queue = connection
            .query_row(
                "SELECT id, name, max_concurrent FROM queues WHERE name = ?1",
                params![name],
                |row| {
                    Ok(Queue {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        max_concurrent: row.get::<_, i64>(2)? as u32,
                    })
                },
            )
            .optional()?;

        queue.ok_or_else(|| StoreError::QueueNotFound(name.to_string()))
    }

    /// Changes the concurrency limit of a queue.
    pub fn set_queue_limit(&self, id: i64, max_concurrent: u32) -> Result<(), StoreError> {
        let connection = self.lock()?;
        let changed = connection.execute(
            "UPDATE queues SET max_concurrent = ?2 WHERE id = ?1",
            params![id, i64::from(max_concurrent)],
        )?;
        if changed == 0 {
            return Err(StoreError::QueueNotFound(id.to_string()));
        }
        Ok(())
    }

    /// Lists the jobs of a queue in execution order.
    pub fn queue_jobs(&self, queue_id: i64) -> Result<Vec<Job>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare(&format!(
            "SELECT {JOB_COLUMNS} FROM jobs WHERE queue_id = ?1 ORDER BY position ASC, id ASC"
        ))?;
        let rows = statement.query_map(params![queue_id], db_job_from_row)?;

        let mut jobs = Vec::new();
        for row in rows {
            jobs.push(to_job(row?)?);
        }
        Ok(jobs)
    }

    /// Moves a job to a new position in its queue.
    pub fn set_job_position(&self, id: JobId, position: i64) -> Result<(), StoreError> {
        let connection = self.lock()?;
        let changed = connection.execute(
            "UPDATE jobs SET position = ?2 WHERE id = ?1",
            params![id.0, position],
        )?;
        if changed == 0 {
            return Err(StoreError::JobNotFound(id));
        }
        Ok(())
    }

    /// Pauses jobs that were running when the process stopped.
    pub fn recover_interrupted(&self) -> Result<usize, StoreError> {
        let connection = self.lock()?;
        let changed = connection.execute(
            "UPDATE jobs SET state = 'paused', updated_at = ?1
             WHERE state IN ('probing', 'downloading', 'verifying')",
            params![now_millis()],
        )?;
        Ok(changed)
    }

    /// Returns the value of a persisted setting by key.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>, StoreError> {
        let connection = self.lock()?;
        let mut statement = connection.prepare("SELECT value FROM settings WHERE key = ?1")?;
        let mut rows = statement.query(params![key])?;
        if let Some(row) = rows.next()? {
            Ok(Some(row.get(0)?))
        } else {
            Ok(None)
        }
    }

    /// Sets or updates a persisted setting.
    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), StoreError> {
        let connection = self.lock()?;
        connection.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Deletes a job and its segments from the database.
    pub fn delete_job(&self, id: JobId) -> Result<(), StoreError> {
        let connection = self.lock()?;
        let affected = connection.execute("DELETE FROM jobs WHERE id = ?1", params![id.0])?;
        if affected == 0 {
            return Err(StoreError::JobNotFound(id));
        }
        Ok(())
    }

    /// Deletes a named queue if it is not the default queue.
    pub fn delete_queue(&self, name: &str) -> Result<(), StoreError> {
        if name == "default" {
            return Err(StoreError::CannotDeleteDefaultQueue);
        }
        let connection = self.lock()?;
        let affected = connection.execute("DELETE FROM queues WHERE name = ?1", params![name])?;
        if affected == 0 {
            return Err(StoreError::QueueNotFound(name.to_string()));
        }
        Ok(())
    }

    fn from_connection(connection: Connection) -> Result<Self, StoreError> {
        connection.pragma_update(None, "foreign_keys", true)?;
        connection.pragma_update(None, "busy_timeout", 5000_i64)?;

        let store = Self {
            connection: Arc::new(Mutex::new(connection)),
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
    #[error("queue not found: {0}")]
    QueueNotFound(String),
    #[error("cannot delete default queue")]
    CannotDeleteDefaultQueue,
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
    queue_id: i64,
    position: i64,
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
        queue_id: row.get(5)?,
        position: row.get(6)?,
        backend: row.get(7)?,
        total_bytes: row.get(8)?,
        downloaded_bytes: row.get(9)?,
        supports_ranges: row.get(10)?,
        etag: row.get(11)?,
        last_modified: row.get(12)?,
        content_type: row.get(13)?,
        error_message: row.get(14)?,
        error_retryable: row.get(15)?,
        options_json: row.get(16)?,
        created_at: row.get(17)?,
        updated_at: row.get(18)?,
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
        queue_id: row.queue_id,
        position: row.position,
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
