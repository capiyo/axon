//! Durable SQLite outbox coordinating authoritative memory metadata with MongoDB.
//!
//! A committed outbox row is recoverable after process restart. The worker uses
//! leases and immutable (memory_id, version) keys so duplicate delivery is safe.

use crate::memory_documents::{MemoryDocument, MemoryDocumentError, MemoryDocumentRepository};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use tokio::{sync::watch, time::sleep};
use uuid::Uuid;

const MAX_ATTEMPTS: i64 = 8;
const LEASE_SECONDS: i64 = 60;

#[derive(Debug, Error)]
pub enum MemoryOutboxError {
    #[error("SQLite memory outbox error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("memory outbox serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("MongoDB memory document error: {0}")]
    Document(#[from] MemoryDocumentError),
    #[error("memory document version conflicts with authoritative SQLite metadata")]
    VersionConflict,
    #[error("memory outbox lease was lost before completion")]
    LeaseLost,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryOutboxEvent {
    pub event_id: Uuid,
    pub document: MemoryDocument,
}

#[derive(Debug, Clone, Serialize)]
pub struct AuthorizedMemoryDocumentVersion {
    pub version: u64,
    pub content_hash: String,
    pub source_event_sequence: u64,
    pub qkv_version: String,
    pub qoin_vector_version: Option<String>,
    pub scope: String,
    pub project_id: Option<Uuid>,
    pub owner_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecordedMemoryFeedback {
    pub feedback_id: Uuid,
    pub memory_id: Uuid,
    pub document_version: u64,
    pub event_type: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryLifecycleAction {
    Archive,
    Restore,
    Delete,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryReconciliationIssue {
    pub memory_id: Uuid,
    pub version: u64,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MemoryReconciliationReport {
    pub workspace_id: Uuid,
    pub sqlite_versions_scanned: usize,
    pub mongo_documents_scanned: usize,
    pub pending_outbox_events: u64,
    pub processing_outbox_events: u64,
    pub dead_letter_outbox_events: u64,
    pub scan_limit_reached: bool,
    pub issues: Vec<MemoryReconciliationIssue>,
}

#[derive(Clone)]
pub struct MemoryOutbox {
    pool: SqlitePool,
}

include!("queue.rs");
include!("lifecycle.rs");
include!("feedback.rs");
include!("reconciliation.rs");
include!("retrieval.rs");
include!("worker.rs");

#[cfg(test)]
include!("tests.rs");
