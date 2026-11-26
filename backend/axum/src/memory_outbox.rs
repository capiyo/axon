//! Durable SQLite outbox coordinating authoritative memory metadata with MongoDB.
//!
//! A committed outbox row is recoverable after process restart. The worker uses
//! leases and immutable (memory_id, version) keys so duplicate delivery is safe.

use crate::memory_documents::{
    MemoryDocument, MemoryDocumentError, MemoryDocumentRepository,
};
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{QueryBuilder, Row, Sqlite, SqlitePool};
use std::{collections::{HashMap, HashSet}, sync::Arc, time::Duration};
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
    pub task_id: Option<Uuid>,}

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

impl MemoryOutbox {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Atomically records an audit event, authoritative metadata, and its
    /// MongoDB write event. The audit sequence becomes the immutable source
    /// sequence stored in both SQLite metadata and the MongoDB document.
    pub async fn enqueue(
        &self,
        document: &mut MemoryDocument,
        actor_user_id: Uuid,
    ) -> Result<(), MemoryOutboxError> {
        document
            .validate()
            .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
        let version = i64::try_from(document.version)
            .map_err(|_| sqlx::Error::Protocol("memory version exceeds SQLite range".into()))?;
        let lifecycle_status = match document.status.as_str() {
            "proposed" => "proposed",
            "archived" => "archived",
            "deleted" => "deleted",
            "ready" => "approved",
            _ => return Err(MemoryOutboxError::VersionConflict),
        };

        let mut transaction = self.pool.begin().await?;
        let existing = sqlx::query(
            "SELECT workspace_id, project_id, owner_id, task_id, scope, document_type, \
             content_hash, source_event_sequence, qkv_version, qoin_vector_version, lifecycle_status, sync_status \
             FROM memory_document_versions WHERE memory_id = ? AND version = ?",
        )
        .bind(document.memory_id.to_string())
        .bind(version)
        .fetch_optional(&mut *transaction)
        .await?;

        if let Some(existing) = existing {
            let stored_workspace: String = existing.try_get("workspace_id")?;
            let stored_project: Option<String> = existing.try_get("project_id")?;
            let stored_owner: Option<String> = existing.try_get("owner_id")?;
            let stored_task: Option<String> = existing.try_get("task_id")?;
            let stored_scope: String = existing.try_get("scope")?;
            let stored_type: String = existing.try_get("document_type")?;
            let stored_hash: String = existing.try_get("content_hash")?;
            let stored_sequence: i64 = existing.try_get("source_event_sequence")?;
            let stored_qkv_version: String = existing.try_get("qkv_version")?;
            let stored_qoin_version: Option<String> = existing.try_get("qoin_vector_version")?;
            let stored_lifecycle: String = existing.try_get("lifecycle_status")?;
            let sync_status: String = existing.try_get("sync_status")?;
            if stored_workspace != document.workspace_id.to_string()
                || stored_project != document.project_id.map(|id| id.to_string())
                || stored_owner != document.owner_id.map(|id| id.to_string())
                || stored_task != document.task_id.map(|id| id.to_string())
                || stored_scope != document.scope
                || stored_type != document.document_type
                || stored_hash != document.content_hash
                || stored_sequence != i64::try_from(document.source_event_sequence)
                    .map_err(|_| sqlx::Error::Protocol("source sequence exceeds SQLite range".into()))?
                || stored_qkv_version != document.qkv_version
                || stored_qoin_version != document.qoin_vector_version
                || stored_lifecycle != lifecycle_status
            {
                return Err(MemoryOutboxError::VersionConflict);
            }

            if sync_status == "pending" || sync_status == "failed" {
                let payload = serde_json::to_string(document)?;
                let now = Utc::now().to_rfc3339();
                sqlx::query(
                    "INSERT INTO memory_document_outbox \
                     (event_id, memory_id, version, event_type, payload_json, status, attempts, \
                      available_at, created_at, updated_at) \
                     VALUES (?, ?, ?, 'memory_document.put', ?, 'pending', 0, ?, ?, ?) \
                     ON CONFLICT(memory_id, version, event_type) DO NOTHING",
                )
                .bind(Uuid::new_v4().to_string())
                .bind(document.memory_id.to_string())
                .bind(version)
                .bind(payload)
                .bind(&now)
                .bind(&now)
                .bind(&now)
                .execute(&mut *transaction)
                .await?;
            }
            transaction.commit().await?;
            return Ok(());
        }

        if document.source_event_sequence != 0 {
            return Err(MemoryOutboxError::VersionConflict);
        }
        let now = Utc::now().to_rfc3339();
        let audit_event_type = match lifecycle_status {
            "proposed" => "memory_document.created",
            "approved" => "memory_document.approved",
            "archived" => "memory_document.archived",
            "deleted" => "memory_document.deleted",
            _ => "memory_document.versioned",
        };
        let audit_result = sqlx::query(
            "INSERT INTO audit_events \
             (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) \
             VALUES (?, ?, ?, ?, 'memory_document', ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(document.workspace_id.to_string())
        .bind(format!("human:{actor_user_id}"))
        .bind(audit_event_type)
        .bind(document.memory_id.to_string())
        .bind(&now)
        .bind(serde_json::json!({
            "memory_id": document.memory_id,
            "version": document.version,
            "content_hash": document.content_hash,
            "lifecycle_status": lifecycle_status
        }).to_string())
        .execute(&mut *transaction)
        .await?;
        let source_sequence = u64::try_from(audit_result.last_insert_rowid())
            .map_err(|_| sqlx::Error::Protocol("invalid audit source sequence".into()))?;
        document.source_event_sequence = source_sequence;
        let source_sequence_sql = i64::try_from(source_sequence)
            .map_err(|_| sqlx::Error::Protocol("source sequence exceeds SQLite range".into()))?;
        let payload = serde_json::to_string(document)?;

        sqlx::query(
            "INSERT INTO memory_document_versions \
             (memory_id, version, workspace_id, project_id, owner_id, task_id, scope, \
              document_type, content_hash, source_event_sequence, qkv_version, \
              qoin_vector_version, lifecycle_status, sync_status, created_at, updated_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)",
        )
        .bind(document.memory_id.to_string())
        .bind(version)
        .bind(document.workspace_id.to_string())
        .bind(document.project_id.map(|id| id.to_string()))
        .bind(document.owner_id.map(|id| id.to_string()))
        .bind(document.task_id.map(|id| id.to_string()))
        .bind(&document.scope)
        .bind(&document.document_type)
        .bind(&document.content_hash)
        .bind(source_sequence_sql)
        .bind(&document.qkv_version)
        .bind(&document.qoin_vector_version)
        .bind(lifecycle_status)
        .bind(document.created_at.to_rfc3339())
        .bind(document.updated_at.to_rfc3339())
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            "INSERT INTO memory_document_outbox \
             (event_id, memory_id, version, event_type, payload_json, status, attempts, \
              available_at, created_at, updated_at) \
             VALUES (?, ?, ?, 'memory_document.put', ?, 'pending', 0, ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(document.memory_id.to_string())
        .bind(version)
        .bind(payload)
        .bind(&now)
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(())
    }

    async fn claim_next(
        &self,
        worker_id: &str,
    ) -> Result<Option<MemoryOutboxEvent>, MemoryOutboxError> {
        let now = Utc::now();
        let now_text = now.to_rfc3339();
        let lease_until = (now + ChronoDuration::seconds(LEASE_SECONDS)).to_rfc3339();
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT event_id, payload_json FROM memory_document_outbox              WHERE payload_json IS NOT NULL AND available_at <= ? AND              (status = 'pending' OR (status = 'processing' AND lease_until <= ?))              ORDER BY created_at ASC, event_id ASC LIMIT 1",
        )
        .bind(&now_text)
        .bind(&now_text)
        .fetch_optional(&mut *transaction)
        .await?;

        let Some(row) = row else {
            transaction.commit().await?;
            return Ok(None);
        };
        let event_id_text: String = row.try_get("event_id")?;
        let payload: Option<String> = row.try_get("payload_json")?;
        let Some(payload) = payload else {
            transaction.commit().await?;
            return Ok(None);
        };
        let document: MemoryDocument = match serde_json::from_str(&payload) {
            Ok(document) => document,
            Err(_) => {
                sqlx::query(
                    "UPDATE memory_document_outbox SET status = 'dead_letter', payload_json = NULL, \
                     last_error = 'invalid outbox payload', updated_at = ? WHERE event_id = ?",
                )
                .bind(&now_text)
                .bind(&event_id_text)
                .execute(&mut *transaction)
                .await?;
                transaction.commit().await?;
                return Ok(None);
            }
        };
        let event_id = Uuid::parse_str(&event_id_text)
            .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
        let changed = sqlx::query(
            "UPDATE memory_document_outbox SET status = 'processing', attempts = attempts + 1,              lease_owner = ?, lease_until = ?, updated_at = ?              WHERE event_id = ? AND available_at <= ? AND              (status = 'pending' OR (status = 'processing' AND lease_until <= ?))",
        )
        .bind(worker_id)
        .bind(&lease_until)
        .bind(&now_text)
        .bind(&event_id_text)
        .bind(&now_text)
        .bind(&now_text)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        if changed.rows_affected() != 1 {
            return Ok(None);
        }
        Ok(Some(MemoryOutboxEvent { event_id, document }))
    }

    async fn complete(
        &self,
        event: &MemoryOutboxEvent,
        worker_id: &str,
    ) -> Result<(), MemoryOutboxError> {
        let now = Utc::now().to_rfc3339();
        let version = i64::try_from(event.document.version)
            .map_err(|_| sqlx::Error::Protocol("memory version exceeds SQLite range".into()))?;
        let mut transaction = self.pool.begin().await?;
        let completed = sqlx::query(
            "UPDATE memory_document_outbox SET status = 'completed', payload_json = NULL,              lease_owner = NULL, lease_until = NULL, last_error = NULL, updated_at = ?              WHERE event_id = ? AND status = 'processing' AND lease_owner = ?",
        )
        .bind(&now)
        .bind(event.event_id.to_string())
        .bind(worker_id)
        .execute(&mut *transaction)
        .await?;
        if completed.rows_affected() != 1 {
            return Err(MemoryOutboxError::LeaseLost);
        }
        let metadata_updated = sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored', updated_at = ?              WHERE memory_id = ? AND version = ? AND content_hash = ?",
        )
        .bind(&now)
        .bind(event.document.memory_id.to_string())
        .bind(version)
        .bind(&event.document.content_hash)
        .execute(&mut *transaction)
        .await?;
        if metadata_updated.rows_affected() != 1 {
            return Err(MemoryOutboxError::VersionConflict);
        }
        transaction.commit().await?;
        Ok(())
    }

    async fn fail(
        &self,
        event_id: Uuid,
        worker_id: &str,
        safe_error: &str,
    ) -> Result<(), MemoryOutboxError> {
        let now = Utc::now();
        let now_text = now.to_rfc3339();
        let row = sqlx::query(
            "SELECT attempts FROM memory_document_outbox              WHERE event_id = ? AND status = 'processing' AND lease_owner = ?",
        )
        .bind(event_id.to_string())
        .bind(worker_id)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Err(MemoryOutboxError::LeaseLost);
        };
        let attempts: i64 = row.try_get("attempts")?;
        let dead = attempts >= MAX_ATTEMPTS;
        let delay_seconds = (2_i64.saturating_pow(attempts.min(10) as u32)).min(3600);
        let available_at = (now + ChronoDuration::seconds(delay_seconds)).to_rfc3339();
        let status = if dead { "dead_letter" } else { "pending" };
        let mut transaction = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE memory_document_outbox SET status = ?, available_at = ?, lease_owner = NULL,              lease_until = NULL, last_error = ?, updated_at = ?              WHERE event_id = ? AND status = 'processing' AND lease_owner = ?",
        )
        .bind(status)
        .bind(available_at)
        .bind(safe_error.chars().take(512).collect::<String>())
        .bind(now_text)
        .bind(event_id.to_string())
        .bind(worker_id)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() != 1 {
            return Err(MemoryOutboxError::LeaseLost);
        }
        if dead {
            sqlx::query(
                "UPDATE memory_document_versions SET sync_status = 'failed', updated_at = ?                  WHERE memory_id = (SELECT memory_id FROM memory_document_outbox WHERE event_id = ?)                  AND version = (SELECT version FROM memory_document_outbox WHERE event_id = ?)",
            )
            .bind(Utc::now().to_rfc3339())
            .bind(event_id.to_string())
            .bind(event_id.to_string())
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    /// Approves a proposed document version in the authoritative SQLite
    /// metadata and appends an audit event in the same transaction.
    pub async fn approve(
        &self,
        workspace_id: Uuid,
        memory_id: Uuid,
        version: u64,
        actor_user_id: Uuid,
    ) -> Result<bool, MemoryOutboxError> {
        let version = i64::try_from(version)
            .map_err(|_| sqlx::Error::Protocol("memory version exceeds SQLite range".into()))?;
        let now = Utc::now().to_rfc3339();
        let mut transaction = self.pool.begin().await?;
        let scope_row = sqlx::query(
            "SELECT scope, project_id FROM memory_document_versions WHERE workspace_id = ? AND memory_id = ? AND version = ? AND lifecycle_status = 'proposed'",
        )
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(version)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(scope_row) = scope_row else {
            transaction.rollback().await?;
            return Ok(false);
        };
        let scope: String = scope_row.try_get("scope")?;
        let project_id: Option<String> = scope_row.try_get("project_id")?;
        if scope == "project_shared" {
            let Some(project_id) = project_id else {
                return Err(MemoryOutboxError::VersionConflict);
            };
            let authorized: Option<i64> = sqlx::query_scalar(
                "SELECT 1 FROM project_memberships pm
                 JOIN projects p ON p.id = pm.project_id
                 JOIN workspace_memberships wm ON wm.workspace_id = p.workspace_id AND wm.user_id = pm.user_id
                 WHERE pm.project_id = ? AND pm.user_id = ?
                   AND pm.revoked_at IS NULL AND p.workspace_id = ?",
            )
            .bind(project_id)
            .bind(actor_user_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_optional(&mut *transaction)
            .await?;
            if authorized.is_none() {
                transaction.rollback().await?;
                return Ok(false);
            }
        }
        let changed = sqlx::query(
            "UPDATE memory_document_versions SET lifecycle_status = 'approved', updated_at = ? \
             WHERE workspace_id = ? AND memory_id = ? AND version = ? AND lifecycle_status = 'proposed'",
        )
        .bind(&now)
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(version)
        .execute(&mut *transaction)
        .await?;
        if changed.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(false);
        }

        sqlx::query(
            "INSERT INTO audit_events \
             (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) \
             VALUES (?, ?, ?, 'memory_document.approved', 'memory_document', ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(workspace_id.to_string())
        .bind(format!("human:{actor_user_id}"))
        .bind(memory_id.to_string())
        .bind(&now)
        .bind(serde_json::json!({"memory_id": memory_id, "version": version}).to_string())
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(true)
    }

    /// Creates a new immutable lifecycle version. The original MongoDB
    /// version and every epoch pin remain untouched; SQLite continues to
    /// decide which version is eligible for current retrieval.
    pub async fn transition_lifecycle(
        &self,
        repository: &dyn MemoryDocumentRepository,
        workspace_id: Uuid,
        memory_id: Uuid,
        version: u64,
        actor_user_id: Uuid,
        action: MemoryLifecycleAction,
    ) -> Result<bool, MemoryOutboxError> {
        let version_sql = i64::try_from(version)
            .map_err(|_| sqlx::Error::Protocol("memory version exceeds SQLite range".into()))?;
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT scope, project_id, owner_id, content_hash, document_type,
                    source_event_sequence, qkv_version, qoin_vector_version,
                    lifecycle_status, sync_status
             FROM memory_document_versions
             WHERE workspace_id = ? AND memory_id = ? AND version = ?",
        )
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(version_sql)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(row) = row else {
            transaction.rollback().await?;
            return Ok(false);
        };
        let scope: String = row.try_get("scope")?;
        let project_id: Option<String> = row.try_get("project_id")?;
        let owner_id: Option<String> = row.try_get("owner_id")?;
        let content_hash: String = row.try_get("content_hash")?;
        let document_type: String = row.try_get("document_type")?;
        let source_sequence: i64 = row.try_get("source_event_sequence")?;
        let qkv_version: String = row.try_get("qkv_version")?;
        let qoin_vector_version: Option<String> = row.try_get("qoin_vector_version")?;
        let lifecycle_status: String = row.try_get("lifecycle_status")?;
        let sync_status: String = row.try_get("sync_status")?;

        let latest_version: i64 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) FROM memory_document_versions WHERE workspace_id = ? AND memory_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .fetch_one(&mut *transaction)
        .await?;
        if latest_version != version_sql || !matches!(sync_status.as_str(), "stored" | "indexed") {
            transaction.rollback().await?;
            return Ok(false);
        }
        let allowed_transition = match action {
            MemoryLifecycleAction::Archive => lifecycle_status == "approved",
            MemoryLifecycleAction::Delete => matches!(lifecycle_status.as_str(), "proposed" | "approved" | "archived"),
            MemoryLifecycleAction::Restore => lifecycle_status == "archived",
        };
        if !allowed_transition {
            transaction.rollback().await?;
            return Ok(false);
        }

        let workspace_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(actor_user_id.to_string())
        .fetch_optional(&mut *transaction)
        .await?;
        if workspace_member.is_none() {
            transaction.rollback().await?;
            return Ok(false);
        }

        match scope.as_str() {
            "project_shared" => {
                let Some(project_id) = project_id.as_deref() else {
                    return Err(MemoryOutboxError::VersionConflict);
                };
                let authorized: Option<i64> = sqlx::query_scalar(
                    "SELECT 1 FROM project_memberships pm
                     JOIN projects p ON p.id = pm.project_id
                     JOIN workspace_memberships wm ON wm.workspace_id = p.workspace_id AND wm.user_id = pm.user_id
                     WHERE pm.project_id = ? AND pm.user_id = ?
                       AND pm.revoked_at IS NULL AND p.workspace_id = ?",
                )
                .bind(project_id)
                .bind(actor_user_id.to_string())
                .bind(workspace_id.to_string())
                .fetch_optional(&mut *transaction)
                .await?;
                if authorized.is_none() {
                    transaction.rollback().await?;
                    return Ok(false);
                }
            }
            "agent_private" => {
                let Some(owner_id) = owner_id.as_deref() else {
                    return Err(MemoryOutboxError::VersionConflict);
                };
                let agent_payload: Option<String> = sqlx::query_scalar(
                    "SELECT payload_json FROM entities WHERE kind = 'agent' AND id = ? AND workspace_id = ?",
                )
                .bind(owner_id)
                .bind(workspace_id.to_string())
                .fetch_optional(&mut *transaction)
                .await?;
                let Some(agent_payload) = agent_payload else {
                    transaction.rollback().await?;
                    return Ok(false);
                };
                let agent = serde_json::from_str::<crate::storage::StoredEntity>(&agent_payload)?;
                let owned = matches!(
                    agent,
                    crate::storage::StoredEntity::Agent(ref value)
                        if value.created_by_user_id == Some(actor_user_id)
                );
                if !owned {
                    transaction.rollback().await?;
                    return Ok(false);
                }
            }
            "task_shared" | "workspace_shared" => {}
            _ => {
                transaction.rollback().await?;
                return Ok(false);
            }
        }
        transaction.commit().await?;

        let mut document = repository
            .get_version(memory_id, version)
            .await?
            .ok_or(MemoryOutboxError::VersionConflict)?;
        document.validate()?;
        if document.workspace_id != workspace_id
            || document.memory_id != memory_id
            || document.version != version
            || document.content_hash != content_hash
            || document.scope != scope
            || document.project_id.map(|id| id.to_string()) != project_id
            || document.owner_id.map(|id| id.to_string()) != owner_id
            || document.document_type != document_type
            || document.source_event_sequence != u64::try_from(source_sequence)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?
            || document.qkv_version != qkv_version
            || document.qoin_vector_version != qoin_vector_version
        {
            return Err(MemoryOutboxError::VersionConflict);
        }

        document.version = version.checked_add(1)
            .ok_or_else(|| sqlx::Error::Protocol("memory version overflow".into()))?;
        document.updated_at = Utc::now();
        document.source_event_sequence = 0;
        document.status = match action {
            MemoryLifecycleAction::Archive => "archived",
            MemoryLifecycleAction::Restore => "ready",
            MemoryLifecycleAction::Delete => "deleted",
        }.into();
        document.content_hash = document.calculate_content_hash();
        self.enqueue(&mut document, actor_user_id).await?;
        Ok(true)
    }

    /// Records feedback only for the exact memory version pinned to the
    /// caller's authenticated agent session. Feedback never mutates a memory
    /// version or an already-published epoch.
    pub async fn record_retrieval_feedback(
        &self,
        workspace_id: Uuid,
        session_id: Uuid,
        agent_id: Uuid,
        actor_user_id: Uuid,
        memory_id: Uuid,
        document_version: u64,
        event_type: &str,
        evidence: &Value,
    ) -> Result<Option<RecordedMemoryFeedback>, MemoryOutboxError> {
        const ALLOWED_EVENTS: &[&str] = &[
            "retrieved", "selected", "ignored", "reused",
            "helpful", "irrelevant", "contradicted", "incorrect",
        ];
        if !ALLOWED_EVENTS.contains(&event_type) {
            return Err(MemoryOutboxError::VersionConflict);
        }
        let evidence_json = serde_json::to_string(evidence)?;
        if evidence_json.len() > 4096 {
            return Err(MemoryOutboxError::VersionConflict);
        }
        let version_sql = i64::try_from(document_version)
            .map_err(|_| sqlx::Error::Protocol("feedback version exceeds SQLite range".into()))?;
        let now = Utc::now();
        let feedback_id = Uuid::new_v4();
        let mut tx = self.pool.begin().await?;
        let workspace_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(actor_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if workspace_member.is_none() {
            tx.rollback().await?;
            return Ok(None);
        }
        let agent_payload: Option<String> = sqlx::query_scalar(
            "SELECT payload_json FROM entities WHERE kind = 'agent' AND id = ? AND workspace_id = ?",
        )
        .bind(agent_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(agent_payload) = agent_payload else {
            tx.rollback().await?;
            return Ok(None);
        };
        let agent_entity: crate::storage::StoredEntity = serde_json::from_str(&agent_payload)?;
        if !matches!(agent_entity, crate::storage::StoredEntity::Agent(ref agent)
            if agent.created_by_user_id == Some(actor_user_id)) {
            tx.rollback().await?;
            return Ok(None);
        }
        let session = sqlx::query(
            "SELECT memory_epoch_id, task_id FROM agent_memory_sessions
             WHERE session_id = ? AND workspace_id = ? AND agent_id = ?
               AND (expires_at IS NULL OR expires_at > ?)",
        )
        .bind(session_id.to_string())
        .bind(workspace_id.to_string())
        .bind(agent_id.to_string())
        .bind(now.to_rfc3339())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(session) = session else {
            tx.rollback().await?;
            return Ok(None);
        };
        let epoch_id: String = session.try_get("memory_epoch_id")?;
        let session_task_id: Option<String> = session.try_get("task_id")?;
        let pinned = sqlx::query(
            "SELECT scope, project_id, owner_id, task_id FROM memory_document_epoch_items
             WHERE epoch_id = ? AND workspace_id = ? AND memory_id = ? AND document_version = ?",
        )
        .bind(epoch_id)
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(version_sql)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(pinned) = pinned else {
            tx.rollback().await?;
            return Ok(None);
        };
        let scope: String = pinned.try_get("scope")?;
        let project_id: Option<String> = pinned.try_get("project_id")?;
        let owner_id: Option<String> = pinned.try_get("owner_id")?;
        let task_id: Option<String> = pinned.try_get("task_id")?;
        let agent_id_text = agent_id.to_string();
        match scope.as_str() {
            "workspace_shared" => {}
            "agent_private" if owner_id.as_deref() == Some(agent_id_text.as_str()) => {}
            "task_shared" if task_id.is_some() && task_id == session_task_id => {}
            "project_shared" => {
                let Some(project_id) = project_id.as_deref() else {
                    return Err(MemoryOutboxError::VersionConflict);
                };
                let membership: Option<i64> = sqlx::query_scalar(
                    "SELECT 1 FROM project_memberships pm
                     JOIN projects p ON p.id = pm.project_id
                     JOIN workspace_memberships wm ON wm.workspace_id = p.workspace_id AND wm.user_id = pm.user_id
                     WHERE pm.project_id = ? AND pm.user_id = ?
                       AND pm.revoked_at IS NULL AND p.workspace_id = ?",
                )
                .bind(project_id)
                .bind(actor_user_id.to_string())
                .bind(workspace_id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
                if membership.is_none() {
                    tx.rollback().await?;
                    return Ok(None);
                }
            }
            _ => {
                tx.rollback().await?;
                return Ok(None);
            }
        }
        sqlx::query(
            "INSERT INTO memory_retrieval_feedback
             (feedback_id, workspace_id, session_id, agent_id, memory_id, document_version,
              actor_user_id, event_type, created_at, evidence_json)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(feedback_id.to_string())
        .bind(workspace_id.to_string())
        .bind(session_id.to_string())
        .bind(agent_id.to_string())
        .bind(memory_id.to_string())
        .bind(version_sql)
        .bind(actor_user_id.to_string())
        .bind(event_type)
        .bind(now.to_rfc3339())
        .bind(evidence_json)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO audit_events
             (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json)
             VALUES (?, ?, ?, 'memory.retrieval_feedback_recorded', 'memory_document', ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(workspace_id.to_string())
        .bind(format!("human:{actor_user_id}"))
        .bind(memory_id.to_string())
        .bind(now.to_rfc3339())
        .bind(serde_json::json!({
            "feedback_id": feedback_id,
            "session_id": session_id,
            "agent_id": agent_id,
            "document_version": document_version,
            "event_type": event_type
        }).to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(RecordedMemoryFeedback {
            feedback_id,
            memory_id,
            document_version,
            event_type: event_type.to_owned(),
            created_at: now,
        }))
    }

    /// Computes smoothed positive/negative feedback signals for exact
    /// document versions. Unrated versions are omitted so their existing
    /// metadata scores remain unchanged.
    pub async fn feedback_signals(
        &self,
        workspace_id: Uuid,
        versions: &[(Uuid, u64)],
    ) -> Result<HashMap<(Uuid, u64), (f32, f32)>, MemoryOutboxError> {
        if versions.is_empty() {
            return Ok(HashMap::new());
        }
        let normalized_versions = versions
            .iter()
            .map(|(memory_id, version)| {
                i64::try_from(*version)
                    .map(|version| (memory_id.to_string(), version))
                    .map_err(|_| sqlx::Error::Protocol("feedback version exceeds SQLite range".into()))
            })
            .collect::<Result<Vec<_>, _>>()?;
        let mut query = QueryBuilder::<Sqlite>::new(
            "SELECT memory_id, document_version, event_type, COUNT(*) AS event_count
             FROM memory_retrieval_feedback WHERE workspace_id = ",
        );
        query.push_bind(workspace_id.to_string());
        query.push(" AND (memory_id, document_version) IN ");
        query.push_tuples(normalized_versions.iter(), |mut row, (memory_id, version)| {
            row.push_bind(memory_id).push_bind(*version);
        });
        query.push(" GROUP BY memory_id, document_version, event_type");
        let rows = query.build().fetch_all(&self.pool).await?;
        let mut counts: HashMap<(Uuid, u64), (u64, u64)> = HashMap::new();
        for row in rows {
            let memory_id_text: String = row.try_get("memory_id")?;
            let memory_id = Uuid::parse_str(&memory_id_text)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            let version_raw: i64 = row.try_get("document_version")?;
            let version = u64::try_from(version_raw)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            let event_type: String = row.try_get("event_type")?;
            let count_raw: i64 = row.try_get("event_count")?;
            let count = u64::try_from(count_raw)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            match event_type.as_str() {
                "helpful" | "selected" | "reused" => {
                    let entry = counts.entry((memory_id, version)).or_default();
                    entry.0 = entry.0.saturating_add(count);
                }
                "irrelevant" | "contradicted" | "incorrect" => {
                    let entry = counts.entry((memory_id, version)).or_default();
                    entry.1 = entry.1.saturating_add(count);
                }
                _ => {}
            }
        }
        Ok(counts.into_iter().map(|(key, (positive, negative))| {
            let total = (positive + negative) as f32;
            let positive_signal = (positive as f32 + 1.0) / (total + 2.0);
            let negative_signal = (negative as f32 + 1.0) / (total + 2.0);
            (key, (positive_signal, negative_signal))
        }).collect())
    }

    /// Returns only approved, synchronized versions visible to this
    /// workspace-owned agent. The query runs against SQLite before MongoDB
    /// content is hydrated by the API layer.
    pub async fn list_authorized_document_versions(
        &self,
        workspace_id: Uuid,
        memory_id: Uuid,
        requester_agent_id: Uuid,
        requester_user_id: Uuid,
        task_id: Option<Uuid>,
        project_id: Option<Uuid>,
        before_version: Option<u64>,
        requested_limit: usize,
    ) -> Result<Vec<AuthorizedMemoryDocumentVersion>, MemoryOutboxError> {
        let limit = i64::try_from(requested_limit.clamp(1, 101))
            .map_err(|_| sqlx::Error::Protocol("version history limit exceeds SQLite range".into()))?;
        let before_version = before_version
            .map(i64::try_from)
            .transpose()
            .map_err(|_| sqlx::Error::Protocol("version cursor exceeds SQLite range".into()))?;
        let rows = sqlx::query(
            "SELECT version, content_hash, source_event_sequence, qkv_version, qoin_vector_version, scope, project_id, owner_id, task_id \
             FROM memory_document_versions \
             WHERE workspace_id = ? AND memory_id = ? AND lifecycle_status = 'approved' \
             AND sync_status IN ('stored', 'indexed') \
             AND (scope = 'workspace_shared' OR (scope = 'agent_private' AND owner_id = ?) \
                  OR (scope = 'task_shared' AND task_id = ?) \
                  OR (scope = 'project_shared' AND project_id = ? AND EXISTS (\
                      SELECT 1 FROM project_memberships pm JOIN projects p ON p.id = pm.project_id \
                      JOIN workspace_memberships wm ON wm.workspace_id = p.workspace_id AND wm.user_id = pm.user_id \
                      WHERE pm.project_id = memory_document_versions.project_id AND pm.user_id = ? \
                      AND pm.revoked_at IS NULL AND p.workspace_id = memory_document_versions.workspace_id))) \
             AND (? IS NULL OR version < ?) ORDER BY version DESC LIMIT ?",
        )
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(requester_agent_id.to_string())
        .bind(task_id.map(|value| value.to_string()))
        .bind(project_id.map(|value| value.to_string()))
        .bind(requester_user_id.to_string())
        .bind(before_version)
        .bind(before_version)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        let mut versions = Vec::with_capacity(rows.len());
        for row in rows {
            let version: i64 = row.try_get("version")?;
            let source_sequence: i64 = row.try_get("source_event_sequence")?;
            let task_id_text: Option<String> = row.try_get("task_id")?;
            versions.push(AuthorizedMemoryDocumentVersion {
                version: u64::try_from(version)
                    .map_err(|error| sqlx::Error::Protocol(error.to_string()))?,
                content_hash: row.try_get("content_hash")?,
                source_event_sequence: u64::try_from(source_sequence)
                    .map_err(|error| sqlx::Error::Protocol(error.to_string()))?,
                qkv_version: row.try_get("qkv_version")?,
                qoin_vector_version: row.try_get("qoin_vector_version")?,
                scope: row.try_get("scope")?,
                project_id: row.try_get::<Option<String>, _>("project_id")?
                    .map(|value| Uuid::parse_str(&value))
                    .transpose()
                    .map_err(|error| sqlx::Error::Protocol(error.to_string()))?,
                owner_id: row.try_get::<Option<String>, _>("owner_id")?
                    .map(|value| Uuid::parse_str(&value))
                    .transpose()
                    .map_err(|error| sqlx::Error::Protocol(error.to_string()))?,
                task_id: task_id_text
                    .map(|value| Uuid::parse_str(&value))
                    .transpose()
                    .map_err(|error| sqlx::Error::Protocol(error.to_string()))?,
            });
        }
        Ok(versions)
    }

    /// Read-only, bounded consistency audit for one workspace. It reports
    /// mismatches but never repairs or exposes document content.
    pub async fn reconcile_workspace(
        &self,
        repository: &dyn MemoryDocumentRepository,
        workspace_id: Uuid,
        requested_limit: usize,
    ) -> Result<MemoryReconciliationReport, MemoryOutboxError> {
        let limit = requested_limit.clamp(1, 5_000);
        let limit_sql = i64::try_from(limit)
            .map_err(|_| sqlx::Error::Protocol("reconciliation limit exceeds SQLite range".into()))?;
        let rows = sqlx::query(
            "SELECT memory_id, version, scope, content_hash, source_event_sequence, qkv_version, qoin_vector_version, sync_status \
             FROM memory_document_versions WHERE workspace_id = ? ORDER BY memory_id ASC, version ASC LIMIT ?",
        )
        .bind(workspace_id.to_string())
        .bind(limit_sql)
        .fetch_all(&self.pool)
        .await?;
        let documents = repository.list_workspace_documents(workspace_id, limit).await?;
        let mut mongo_by_key = HashMap::new();
        let mut issues = Vec::new();

        for document in &documents {
            let key = (document.memory_id, document.version);
            if mongo_by_key.insert(key, document).is_some() {
                issues.push(MemoryReconciliationIssue {
                    memory_id: document.memory_id,
                    version: document.version,
                    kind: "duplicate_mongo_version".into(),
                });
            }
            if document.validate().is_err() {
                issues.push(MemoryReconciliationIssue {
                    memory_id: document.memory_id,
                    version: document.version,
                    kind: "invalid_mongo_document".into(),
                });
            }
        }

        let mut sqlite_keys = HashSet::with_capacity(rows.len());
        for row in &rows {
            let memory_id_text: String = row.try_get("memory_id")?;
            let memory_id = Uuid::parse_str(&memory_id_text)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            let version_sql: i64 = row.try_get("version")?;
            let version = u64::try_from(version_sql)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            let key = (memory_id, version);
            sqlite_keys.insert(key);
            let scope: String = row.try_get("scope")?;
            let content_hash: String = row.try_get("content_hash")?;
            let source_sequence: i64 = row.try_get("source_event_sequence")?;
            let qkv_version: String = row.try_get("qkv_version")?;
            let qoin_version: Option<String> = row.try_get("qoin_vector_version")?;
            let sync_status: String = row.try_get("sync_status")?;

            let Some(document) = mongo_by_key.get(&key) else {
                issues.push(MemoryReconciliationIssue {
                    memory_id,
                    version,
                    kind: match sync_status.as_str() {
                        "pending" => "mongo_document_pending_delivery",
                        "failed" => "mongo_document_missing_after_failure",
                        _ => "sqlite_version_missing_in_mongo",
                    }.into(),
                });
                continue;
            };

            let source_sequence = u64::try_from(source_sequence)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            let mut add_issue = |kind: &str| {
                issues.push(MemoryReconciliationIssue {
                    memory_id,
                    version,
                    kind: kind.to_owned(),
                });
            };
            if document.workspace_id != workspace_id {
                add_issue("workspace_id_mismatch");
            }
            if document.scope != scope {
                add_issue("scope_mismatch");
            }
            if document.content_hash != content_hash {
                add_issue("content_hash_mismatch");
            }
            if document.source_event_sequence != source_sequence {
                add_issue("source_event_sequence_mismatch");
            }
            if document.qkv_version != qkv_version {
                add_issue("qkv_model_version_mismatch");
            }
            if document.qoin_vector_version != qoin_version {
                add_issue("qoin_vector_version_mismatch");
            }
            match document.key_metadata.get("qkv_key_version").and_then(serde_json::Value::as_str) {
                Some(key_version) if key_version == document.qkv_version => {}
                Some(_) => add_issue("qkv_key_version_mismatch"),
                None => add_issue("persisted_qkv_key_missing"),
            }
        }

        for document in &documents {
            if !sqlite_keys.contains(&(document.memory_id, document.version)) {
                issues.push(MemoryReconciliationIssue {
                    memory_id: document.memory_id,
                    version: document.version,
                    kind: "mongo_document_without_sqlite_authority".into(),
                });
            }
        }

        let status_rows = sqlx::query(
            "SELECT o.status, COUNT(*) AS count FROM memory_document_outbox o \
             JOIN memory_document_versions v ON v.memory_id = o.memory_id AND v.version = o.version \
             WHERE v.workspace_id = ? GROUP BY o.status",
        )
        .bind(workspace_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let mut pending = 0_u64;
        let mut processing = 0_u64;
        let mut dead_letter = 0_u64;
        for row in status_rows {
            let status: String = row.try_get("status")?;
            let count: i64 = row.try_get("count")?;
            let count = u64::try_from(count)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            match status.as_str() {
                "pending" => pending = count,
                "processing" => processing = count,
                "dead_letter" => dead_letter = count,
                _ => {}
            }
        }

        Ok(MemoryReconciliationReport {
            workspace_id,
            sqlite_versions_scanned: rows.len(),
            mongo_documents_scanned: documents.len(),
            pending_outbox_events: pending,
            processing_outbox_events: processing,
            dead_letter_outbox_events: dead_letter,
            scan_limit_reached: rows.len() == limit || documents.len() == limit,
            issues,
        })
    }

    /// Requeues a valid dead-letter event without changing the immutable
    /// document version. The operation and its audit event are atomic in SQLite.
    pub async fn retry_dead_letter(
        &self,
        workspace_id: Uuid,
        memory_id: Uuid,
        version: u64,
        actor_user_id: Uuid,
    ) -> Result<bool, MemoryOutboxError> {
        let version = i64::try_from(version)
            .map_err(|_| sqlx::Error::Protocol("memory version exceeds SQLite range".into()))?;
        let now = Utc::now().to_rfc3339();
        let mut transaction = self.pool.begin().await?;
        let payload_row = sqlx::query(
            "SELECT payload_json FROM memory_document_outbox \
             WHERE memory_id = ? AND version = ? AND status = 'dead_letter'",
        )
        .bind(memory_id.to_string())
        .bind(version)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(payload_row) = payload_row else {
            transaction.rollback().await?;
            return Ok(false);
        };
        let payload_json: Option<String> = payload_row.try_get("payload_json")?;
        let Some(payload_json) = payload_json else {
            transaction.rollback().await?;
            return Ok(false);
        };
        let document = match serde_json::from_str::<MemoryDocument>(&payload_json) {
            Ok(document) => document,
            Err(_) => {
                transaction.rollback().await?;
                return Ok(false);
            }
        };
        let metadata_row = sqlx::query(
            "SELECT workspace_id, content_hash, sync_status FROM memory_document_versions \
             WHERE workspace_id = ? AND memory_id = ? AND version = ?",
        )
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(version)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(metadata_row) = metadata_row else {
            transaction.rollback().await?;
            return Ok(false);
        };
        let stored_workspace: String = metadata_row.try_get("workspace_id")?;
        let stored_hash: String = metadata_row.try_get("content_hash")?;
        let sync_status: String = metadata_row.try_get("sync_status")?;
        if document.validate().is_err()
            || document.memory_id != memory_id
            || document.workspace_id != workspace_id
            || document.version != version as u64
            || stored_workspace != workspace_id.to_string()
            || stored_hash != document.content_hash
            || sync_status != "failed"
        {
            transaction.rollback().await?;
            return Ok(false);
        }

        let updated = sqlx::query(
            "UPDATE memory_document_outbox SET status = 'pending', attempts = 0, available_at = ?, \
             lease_owner = NULL, lease_until = NULL, last_error = NULL, updated_at = ? \
             WHERE memory_id = ? AND version = ? AND status = 'dead_letter' AND payload_json IS NOT NULL",
        )
        .bind(&now)
        .bind(&now)
        .bind(memory_id.to_string())
        .bind(version)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() != 1 {
            transaction.rollback().await?;
            return Ok(false);
        }

        let metadata = sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'pending', updated_at = ? \
             WHERE workspace_id = ? AND memory_id = ? AND version = ? AND sync_status = 'failed'",
        )
        .bind(&now)
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(version)
        .execute(&mut *transaction)
        .await?;
        if metadata.rows_affected() != 1 {
            return Err(MemoryOutboxError::VersionConflict);
        }

        sqlx::query(
            "INSERT INTO audit_events \
             (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) \
             VALUES (?, ?, ?, 'memory_document.outbox_requeued', 'memory_document', ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(workspace_id.to_string())
        .bind(format!("human:{actor_user_id}"))
        .bind(memory_id.to_string())
        .bind(&now)
        .bind(serde_json::json!({"memory_id": memory_id, "version": version}).to_string())
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(true)
    }

    /// Lists only approved, synchronized document versions visible to an
    /// agent/task pair. Project-scoped documents remain excluded until project
    /// membership authorization is available.
    pub async fn list_authorized_document_refs(
        &self,
        workspace_id: Uuid,
        requester_agent_id: Uuid,
        requester_user_id: Uuid,
        task_id: Option<Uuid>,
        project_id: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<(Uuid, u64)>, MemoryOutboxError> {
        let task_id_text = task_id.map(|id| id.to_string());
        let project_id_text = project_id.map(|id| id.to_string());
        let limit = i64::try_from(limit.clamp(1, 200))
            .map_err(|_| sqlx::Error::Protocol("retrieval limit exceeds SQLite range".into()))?;
        let rows = sqlx::query(
            "SELECT v.memory_id, v.version FROM memory_document_versions v \
             WHERE v.workspace_id = ? AND v.lifecycle_status = 'approved' \
             AND v.sync_status IN ('stored', 'indexed') \
             AND (v.scope = 'workspace_shared' OR (v.scope = 'agent_private' AND v.owner_id = ?) \
             OR (v.scope = 'task_shared' AND v.task_id = ?) \
             OR (v.scope = 'project_shared' AND v.project_id = ? AND EXISTS (\
                 SELECT 1 FROM project_memberships pm JOIN projects p ON p.id = pm.project_id \
                 JOIN workspace_memberships wm ON wm.workspace_id = p.workspace_id AND wm.user_id = pm.user_id \
                 WHERE pm.project_id = v.project_id AND pm.user_id = ? AND pm.revoked_at IS NULL \
                 AND p.workspace_id = v.workspace_id))) \
             AND NOT EXISTS (SELECT 1 FROM memory_document_versions newer \
                 WHERE newer.workspace_id = v.workspace_id AND newer.memory_id = v.memory_id \
                 AND newer.version > v.version \
                 AND (newer.lifecycle_status IN ('archived', 'deleted', 'rejected', 'superseded') \
                      OR (newer.lifecycle_status = 'approved' AND newer.sync_status IN ('stored', 'indexed')))) \
             ORDER BY v.updated_at DESC, v.memory_id ASC LIMIT ?",
        )
        .bind(workspace_id.to_string())
        .bind(requester_agent_id.to_string())
        .bind(task_id_text)
        .bind(project_id_text)
        .bind(requester_user_id.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let memory_id: String = row.try_get("memory_id")?;
                let version: i64 = row.try_get("version")?;
                let memory_id = Uuid::parse_str(&memory_id)
                    .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
                let version = u64::try_from(version)
                    .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
                Ok((memory_id, version))
            })
            .collect()
    }

    /// Returns a MongoDB document only when SQLite confirms its workspace,
    /// approval state, synchronization state, and agent/task visibility.
    pub async fn get_authorized_document(
        &self,
        repository: &dyn MemoryDocumentRepository,
        workspace_id: Uuid,
        memory_id: Uuid,
        version: u64,
        requester_agent_id: Uuid,
        requester_user_id: Uuid,
        task_id: Option<Uuid>,
    ) -> Result<Option<MemoryDocument>, MemoryOutboxError> {
        let version_sql = i64::try_from(version)
            .map_err(|_| sqlx::Error::Protocol("memory version exceeds SQLite range".into()))?;
        let row = sqlx::query(
            "SELECT scope, owner_id, task_id, project_id, document_type, content_hash, \
             source_event_sequence, qkv_version, qoin_vector_version, lifecycle_status, sync_status \
             FROM memory_document_versions WHERE workspace_id = ? AND memory_id = ? AND version = ?",
        )
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(version_sql)
        .fetch_optional(&self.pool)
        .await?;
        let Some(row) = row else {
            return Ok(None);
        };
        let scope: String = row.try_get("scope")?;
        let owner_id: Option<String> = row.try_get("owner_id")?;
        let stored_task_id: Option<String> = row.try_get("task_id")?;
        let stored_project_id: Option<String> = row.try_get("project_id")?;
        let stored_document_type: String = row.try_get("document_type")?;
        let expected_hash: String = row.try_get("content_hash")?;
        let stored_source_sequence: i64 = row.try_get("source_event_sequence")?;
        let stored_qkv_version: String = row.try_get("qkv_version")?;
        let stored_qoin_version: Option<String> = row.try_get("qoin_vector_version")?;
        let lifecycle_status: String = row.try_get("lifecycle_status")?;
        let sync_status: String = row.try_get("sync_status")?;
        if lifecycle_status != "approved" || !matches!(sync_status.as_str(), "stored" | "indexed") {
            return Ok(None);
        }
        let superseded_by_lifecycle: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM memory_document_versions newer
             WHERE newer.workspace_id = ? AND newer.memory_id = ? AND newer.version > ?
               AND (newer.lifecycle_status IN ('archived', 'deleted', 'rejected', 'superseded')
                    OR (newer.lifecycle_status = 'approved' AND newer.sync_status IN ('stored', 'indexed')))
             LIMIT 1",
        )
        .bind(workspace_id.to_string())
        .bind(memory_id.to_string())
        .bind(version_sql)
        .fetch_optional(&self.pool)
        .await?;
        if superseded_by_lifecycle.is_some() {
            return Ok(None);
        }

        let requester_agent_id_text = requester_agent_id.to_string();
        let requester_task_id_text = task_id.map(|id| id.to_string());
        let visible = match scope.as_str() {
            "workspace_shared" => true,
            "agent_private" => owner_id.as_deref() == Some(requester_agent_id_text.as_str()),
            "task_shared" => {
                task_id.is_some()
                    && stored_task_id.as_deref() == requester_task_id_text.as_deref()
            }
            "project_shared" => {
                if let Some(project_id) = stored_project_id.as_deref() {
                    let membership: Option<i64> = sqlx::query_scalar(
                        "SELECT 1 FROM project_memberships pm
                         JOIN projects p ON p.id = pm.project_id
                         JOIN workspace_memberships wm ON wm.workspace_id = p.workspace_id AND wm.user_id = pm.user_id
                         WHERE pm.project_id = ? AND pm.user_id = ?
                           AND pm.revoked_at IS NULL AND p.workspace_id = ?",
                    )
                    .bind(project_id)
                    .bind(requester_user_id.to_string())
                    .bind(workspace_id.to_string())
                    .fetch_optional(&self.pool)
                    .await?;
                    membership.is_some()
                } else {
                    false
                }
            },
            _ => false,
        };
        if !visible {
            return Ok(None);
        }

        let Some(document) = repository.get_version(memory_id, version).await? else {
            return Err(MemoryOutboxError::VersionConflict);
        };
        document.validate()?;
        if document.memory_id != memory_id
            || document.version != version
            || document.workspace_id != workspace_id
            || document.content_hash != expected_hash
            || document.scope != scope
            || document.document_type != stored_document_type
            || document.source_event_sequence != u64::try_from(stored_source_sequence)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?
            || document.qkv_version != stored_qkv_version
            || document.qoin_vector_version != stored_qoin_version
            || document.project_id.map(|id| id.to_string()) != stored_project_id
            || document.owner_id.map(|id| id.to_string()) != owner_id
            || document.task_id.map(|id| id.to_string()) != stored_task_id
        {
            return Err(MemoryOutboxError::VersionConflict);
        }
        // Reflect the authoritative SQLite approval state in the returned view;
        // MongoDB's stored status is intentionally not an authorization source.
        let mut document = document;
        document.status = "ready".into();
        Ok(Some(document))
    }

    /// Hydrates only the exact MongoDB document versions pinned by an immutable
    /// memory epoch. Current lifecycle changes do not rewrite historical epoch
    /// membership; SQLite epoch pins remain the authority for this snapshot.
    pub async fn list_epoch_authorized_documents(
        &self,
        repository: &dyn MemoryDocumentRepository,
        workspace_id: Uuid,
        epoch_id: Uuid,
        requester_agent_id: Uuid,
        requester_user_id: Uuid,
        task_id: Option<Uuid>,
        limit: usize,
    ) -> Result<Option<Vec<MemoryDocument>>, MemoryOutboxError> {
        let epoch_exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM memory_epochs WHERE id = ? AND workspace_id = ?",
        )
        .bind(epoch_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        if epoch_exists.is_none() {
            return Ok(None);
        }

        let limit = i64::try_from(limit.clamp(1, 100))
            .map_err(|_| sqlx::Error::Protocol("epoch document limit exceeds SQLite range".into()))?;
        let rows = sqlx::query(
            "SELECT e.memory_id, e.document_version, e.content_hash, e.scope, e.project_id, e.task_id, e.owner_id, \
                    e.source_event_sequence, e.qkv_version, e.qoin_vector_version \
             FROM memory_document_epoch_items e \
             WHERE e.epoch_id = ? AND e.workspace_id = ? \
             AND NOT EXISTS (SELECT 1 FROM memory_document_epoch_items newer \
                 WHERE newer.epoch_id = e.epoch_id AND newer.memory_id = e.memory_id \
                 AND newer.document_version > e.document_version) \
             ORDER BY e.source_event_sequence DESC, e.memory_id ASC LIMIT ?",
        )
        .bind(epoch_id.to_string())
        .bind(workspace_id.to_string())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut documents = Vec::with_capacity(rows.len());
        let requester_agent_id_text = requester_agent_id.to_string();
        let requester_user_id_text = requester_user_id.to_string();
        let requester_task_id_text = task_id.map(|id| id.to_string());
        for row in rows {
            let memory_id_text: String = row.try_get("memory_id")?;
            let memory_id = Uuid::parse_str(&memory_id_text)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            let version_raw: i64 = row.try_get("document_version")?;
            let version = u64::try_from(version_raw)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            let expected_hash: String = row.try_get("content_hash")?;
            let scope: String = row.try_get("scope")?;
            let project_id: Option<String> = row.try_get("project_id")?;
            let stored_task_id: Option<String> = row.try_get("task_id")?;
            let owner_id: Option<String> = row.try_get("owner_id")?;
            let source_sequence_raw: i64 = row.try_get("source_event_sequence")?;
            let source_sequence = u64::try_from(source_sequence_raw)
                .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
            let qkv_version: String = row.try_get("qkv_version")?;
            let qoin_vector_version: Option<String> = row.try_get("qoin_vector_version")?;

            let visible = match scope.as_str() {
                "workspace_shared" => true,
                "agent_private" => owner_id.as_deref() == Some(requester_agent_id_text.as_str()),
                "task_shared" => task_id.is_some()
                    && stored_task_id.as_deref() == requester_task_id_text.as_deref(),
                "project_shared" => {
                    if let Some(project_id) = project_id.as_deref() {
                        let membership: Option<i64> = sqlx::query_scalar(
                            "SELECT 1 FROM project_memberships pm
                             JOIN projects p ON p.id = pm.project_id
                             JOIN workspace_memberships wm ON wm.workspace_id = p.workspace_id AND wm.user_id = pm.user_id
                             WHERE pm.project_id = ? AND pm.user_id = ?
                               AND pm.revoked_at IS NULL AND p.workspace_id = ?",
                        )
                        .bind(project_id)
                        .bind(&requester_user_id_text)
                        .bind(workspace_id.to_string())
                        .fetch_optional(&self.pool)
                        .await?;
                        membership.is_some()
                    } else {
                        false
                    }
                },
                _ => false,
            };
            if !visible {
                continue;
            }

            // Confirm the authoritative version record still agrees with the
            // immutable epoch before hydrating its MongoDB value.
            let metadata = sqlx::query(
                "SELECT content_hash, sync_status FROM memory_document_versions \
                 WHERE workspace_id = ? AND memory_id = ? AND version = ?",
            )
            .bind(workspace_id.to_string())
            .bind(memory_id.to_string())
            .bind(version_raw)
            .fetch_optional(&self.pool)
            .await?;
            let Some(metadata) = metadata else {
                return Err(MemoryOutboxError::VersionConflict);
            };
            let current_hash: String = metadata.try_get("content_hash")?;
            let sync_status: String = metadata.try_get("sync_status")?;
            if current_hash != expected_hash || !matches!(sync_status.as_str(), "stored" | "indexed") {
                return Err(MemoryOutboxError::VersionConflict);
            }

            let Some(mut document) = repository.get_version(memory_id, version).await? else {
                return Err(MemoryOutboxError::VersionConflict);
            };
            document.validate()?;
            if document.memory_id != memory_id
                || document.version != version
                || document.workspace_id != workspace_id
                || document.content_hash != expected_hash
                || document.scope != scope
                || document.project_id.map(|id| id.to_string()) != project_id
                || document.task_id.map(|id| id.to_string()) != stored_task_id
                || document.owner_id.map(|id| id.to_string()) != owner_id
                || document.source_event_sequence != source_sequence
                || document.qkv_version != qkv_version
                || document.qoin_vector_version != qoin_vector_version
            {
                return Err(MemoryOutboxError::VersionConflict);
            }
            document.status = "ready".into();
            documents.push(document);
        }
        Ok(Some(documents))
    }

    /// Resolves the immutable epoch from the agent's persisted session before
    /// hydrating MongoDB values. Callers cannot substitute an arbitrary epoch.
    pub async fn list_session_authorized_documents(
        &self,
        repository: &dyn MemoryDocumentRepository,
        workspace_id: Uuid,
        session_id: Uuid,
        requester_agent_id: Uuid,
        requester_user_id: Uuid,
        limit: usize,
    ) -> Result<Option<(Uuid, Option<Uuid>, Option<String>, Vec<MemoryDocument>)>, MemoryOutboxError> {
        let session = sqlx::query(
            "SELECT s.memory_epoch_id, s.task_id, \
             (SELECT d.qoin_scoring_version FROM memory_document_epoch_items d \
              WHERE d.epoch_id = s.memory_epoch_id ORDER BY d.memory_id LIMIT 1) AS qoin_scoring_version \
             FROM agent_memory_sessions s \
             WHERE s.session_id = ? AND s.workspace_id = ? AND s.agent_id = ?",
        )
        .bind(session_id.to_string())
        .bind(workspace_id.to_string())
        .bind(requester_agent_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        let Some(session) = session else {
            return Ok(None);
        };
        let epoch_id_text: String = session.try_get("memory_epoch_id")?;
        let epoch_id = Uuid::parse_str(&epoch_id_text)
            .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
        let qoin_scoring_version: Option<String> = session.try_get("qoin_scoring_version")?;
        let distinct_policy_versions: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT qoin_scoring_version) FROM memory_document_epoch_items WHERE epoch_id = ?",
        )
        .bind(epoch_id.to_string())
        .fetch_one(&self.pool)
        .await?;
        if distinct_policy_versions > 1 {
            return Err(MemoryOutboxError::VersionConflict);
        }
        let task_id_text: Option<String> = session.try_get("task_id")?;
        let task_id = task_id_text
            .map(|value| Uuid::parse_str(&value))
            .transpose()
            .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
        let Some(documents) = self
            .list_epoch_authorized_documents(
                repository,
                workspace_id,
                epoch_id,
                requester_agent_id,
                requester_user_id,
                task_id,
                limit,
            )
            .await?
        else {
            return Err(MemoryOutboxError::VersionConflict);
        };
        Ok(Some((epoch_id, task_id, qoin_scoring_version, documents)))
    }

    pub async fn mark_indexed(
        &self,
        memory_id: Uuid,
        version: u64,
        expected_hash: &str,
    ) -> Result<(), MemoryOutboxError> {
        let version = i64::try_from(version)
            .map_err(|_| sqlx::Error::Protocol("memory version exceeds SQLite range".into()))?;
        let result = sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'indexed', updated_at = ?              WHERE memory_id = ? AND version = ? AND content_hash = ? AND sync_status = 'stored'",
        )
        .bind(Utc::now().to_rfc3339())
        .bind(memory_id.to_string())
        .bind(version)
        .bind(expected_hash)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() != 1 {
            return Err(MemoryOutboxError::VersionConflict);
        }
        Ok(())
    }

    /// Polls durable work until the supplied shutdown channel is closed or set.
    pub async fn run(
        self,
        repository: Arc<dyn MemoryDocumentRepository>,
        worker_id: String,
        mut shutdown: watch::Receiver<bool>,
    ) {
        loop {
            if *shutdown.borrow() {
                break;
            }
            match self.claim_next(&worker_id).await {
                Ok(Some(event)) => {
                    let result: Result<(), MemoryOutboxError> = async {
                        repository.put_version(event.document.clone()).await?;
                        let verified = repository
                            .verify_version(
                                event.document.memory_id,
                                event.document.version,
                                &event.document.content_hash,
                            )
                            .await?;
                        if !verified {
                            return Err(MemoryOutboxError::VersionConflict);
                        }
                        self.complete(&event, &worker_id).await
                    }
                    .await;
                    if let Err(error) = result {
                        tracing::warn!(
                            event_id = %event.event_id,
                            error = %error,
                            "memory document outbox event failed"
                        );
                        if let Err(fail_error) = self
                            .fail(
                                event.event_id,
                                &worker_id,
                                "MongoDB persistence or verification failed",
                            )
                            .await
                        {
                            tracing::error!(
                                event_id = %event.event_id,
                                error = %fail_error,
                                "could not update memory outbox retry state"
                            );
                        }
                    }
                }
                Ok(None) => {
                    tokio::select! {
                        _ = sleep(Duration::from_millis(500)) => {},
                        changed = shutdown.changed() => {
                            if changed.is_err() || *shutdown.borrow() {
                                break;
                            }
                        }
                    }
                }
                Err(error) => {
                    tracing::error!(error = %error, "memory outbox polling failed");
                    tokio::select! {
                        _ = sleep(Duration::from_secs(2)) => {},
                        changed = shutdown.changed() => {
                            if changed.is_err() || *shutdown.borrow() {
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{SqliteStorage, Storage};

    pub(super) fn document(workspace_id: Uuid, owner_agent_id: Uuid) -> MemoryDocument {
        let now = Utc::now();
        let mut document = MemoryDocument {
            memory_id: Uuid::new_v4(),
            workspace_id,
            project_id: None,
            task_id: None,
            owner_id: Some(owner_agent_id),
            scope: "agent_private".into(),
            document_type: "technical_decision".into(),
            version: 1,
            title: "Private agent memory".into(),
            content: "Keep this decision in the owning agent scope.".into(),
            content_format: "text/markdown".into(),
            tags: vec!["private".into()],
            source: "test".into(),
            source_kind: "memory_entry".into(),
            source_event_sequence: 0,
            created_at: now,
            updated_at: now,
            content_hash: String::new(),
            key_metadata: serde_json::json!({}),
            value_payload: serde_json::json!({}),
            qkv_version: "axon-lexical-qkv-v1".into(),
            qoin_vector_version: None,
            status: "proposed".into(),
            retention_policy: None,
        };
        document.content_hash = document.calculate_content_hash();
        document
    }

    #[tokio::test]
    async fn outbox_enqueue_is_idempotent_and_approval_is_audited() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let outbox = storage.memory_outbox();
        let workspace_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();
        let actor_id = Uuid::new_v4();
        let mut document = document(workspace_id, agent_id);

        outbox.enqueue(&mut document, actor_id).await.unwrap();
        let first_source_sequence = document.source_event_sequence;
        assert!(first_source_sequence > 0);
        outbox.enqueue(&mut document, actor_id).await.unwrap();

        let event_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM memory_document_outbox WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(event_count, 1);

        assert!(outbox
            .approve(workspace_id, document.memory_id, 1, actor_id)
            .await
            .unwrap());
        let audit_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_events WHERE event_type IN ('memory_document.created', 'memory_document.approved') AND entity_id = ?",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(audit_count, 2);

        let stored_sequence: i64 = sqlx::query_scalar(
            "SELECT source_event_sequence FROM memory_document_versions WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(u64::try_from(stored_sequence).unwrap(), first_source_sequence);

        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        let visible = outbox
            .list_authorized_document_refs(workspace_id, agent_id, Uuid::nil(), None, None, 20)
            .await
            .unwrap();
        let hidden = outbox
            .list_authorized_document_refs(workspace_id, Uuid::new_v4(), Uuid::nil(), None, None, 20)
            .await
            .unwrap();
        assert_eq!(visible, vec![(document.memory_id, 1)]);
        assert!(hidden.is_empty());

        let coordinator = storage.memory_epoch_coordinator();
        assert_eq!(coordinator.publish_due_epochs().await.unwrap(), 1);
        let pinned = sqlx::query(
            "SELECT document_version, content_hash, source_event_sequence, qkv_version, qoin_scoring_version \
             FROM memory_document_epoch_items WHERE memory_id = ? AND document_version = 1",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let pinned_version: i64 = pinned.try_get("document_version").unwrap();
        let pinned_hash: String = pinned.try_get("content_hash").unwrap();
        let pinned_sequence: i64 = pinned.try_get("source_event_sequence").unwrap();
        let pinned_qkv: String = pinned.try_get("qkv_version").unwrap();
        let pinned_qoin: String = pinned.try_get("qoin_scoring_version").unwrap();
        assert_eq!(pinned_version, 1);
        assert_eq!(pinned_hash, document.content_hash);
        assert_eq!(u64::try_from(pinned_sequence).unwrap(), first_source_sequence);
        assert_eq!(pinned_qkv, "axon-lexical-qkv-v1");
        assert_eq!(pinned_qoin, crate::memory_intelligence::QOIN_MEMORY_SCORING_VERSION);
        let update_result = sqlx::query(
            "UPDATE memory_document_epoch_items SET content_hash = 'tampered' WHERE memory_id = ?",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await;
        assert!(update_result.is_err());
        let delete_result = sqlx::query(
            "DELETE FROM memory_document_epoch_items WHERE memory_id = ?",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await;
        assert!(delete_result.is_err());
    }
}


#[cfg(test)]
mod epoch_document_tests {
    use super::*;
    use crate::storage::{SqliteStorage, Storage};
    use std::{collections::HashMap, sync::Mutex};

    #[derive(Default)]
    struct FakeDocumentRepository {
        documents: Mutex<HashMap<(Uuid, u64), MemoryDocument>>,
    }

    #[async_trait::async_trait]
    impl MemoryDocumentRepository for FakeDocumentRepository {
        async fn put_version(&self, document: MemoryDocument) -> Result<(), MemoryDocumentError> {
            document.validate()?;
            self.documents
                .lock()
                .map_err(|_| MemoryDocumentError::InvalidDocument)?
                .insert((document.memory_id, document.version), document);
            Ok(())
        }

        async fn get_version(
            &self,
            memory_id: Uuid,
            version: u64,
        ) -> Result<Option<MemoryDocument>, MemoryDocumentError> {
            Ok(self.documents
                .lock()
                .map_err(|_| MemoryDocumentError::InvalidDocument)?
                .get(&(memory_id, version))
                .cloned())
        }

        async fn list_versions(
            &self,
            memory_id: Uuid,
        ) -> Result<Vec<MemoryDocument>, MemoryDocumentError> {
            let mut versions = self.documents
                .lock()
                .map_err(|_| MemoryDocumentError::InvalidDocument)?
                .values()
                .filter(|document| document.memory_id == memory_id)
                .cloned()
                .collect::<Vec<_>>();
            versions.sort_by_key(|document| document.version);
            Ok(versions)
        }

        async fn list_workspace_documents(
            &self,
            workspace_id: Uuid,
            limit: usize,
        ) -> Result<Vec<MemoryDocument>, MemoryDocumentError> {
            let mut documents = self.documents
                .lock()
                .map_err(|_| MemoryDocumentError::InvalidDocument)?
                .values()
                .filter(|document| document.workspace_id == workspace_id)
                .cloned()
                .collect::<Vec<_>>();
            documents.sort_by(|left, right| {
                left.memory_id.as_bytes().cmp(right.memory_id.as_bytes())
                    .then_with(|| left.version.cmp(&right.version))
            });
            documents.truncate(limit.clamp(1, 5_000));
            Ok(documents)
        }

        async fn verify_version(
            &self,
            memory_id: Uuid,
            version: u64,
            expected_hash: &str,
        ) -> Result<bool, MemoryDocumentError> {
            Ok(self.get_version(memory_id, version).await?
                .is_some_and(|document| document.content_hash == expected_hash))
        }

        async fn ping(&self) -> Result<(), MemoryDocumentError> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn project_scoped_memory_requires_active_project_membership_for_search_and_direct_reads() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let owner = storage.ensure_firebase_user("axon-test", "project-memory-owner").await.unwrap();
        let member = storage.ensure_firebase_user("axon-test", "project-memory-member").await.unwrap();
        storage.add_workspace_member(owner.workspace.id, owner.user_id, member.user_id).await.unwrap();
        let project = storage.create_project_for_user(owner.workspace.id, owner.user_id, "Project memory".into()).await.unwrap();
        storage.set_project_member(
            owner.workspace.id, project.id, owner.user_id, member.user_id, crate::model::ProjectRole::Member,
        ).await.unwrap();

        let outbox = storage.memory_outbox();
        let repository = FakeDocumentRepository::default();
        let agent_id = Uuid::new_v4();
        let mut document = super::tests::document(owner.workspace.id, agent_id);
        document.scope = "project_shared".into();
        document.project_id = Some(project.id);
        document.owner_id = None;
        document.status = "proposed".into();
        outbox.enqueue(&mut document, owner.user_id).await.unwrap();
        repository.put_version(document.clone()).await.unwrap();
        assert!(outbox.approve(owner.workspace.id, document.memory_id, 1, owner.user_id).await.unwrap());
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        let visible = outbox.list_authorized_document_refs(
            owner.workspace.id, agent_id, member.user_id, None, Some(project.id), 20,
        ).await.unwrap();
        assert_eq!(visible, vec![(document.memory_id, 1)]);
        let without_project = outbox.list_authorized_document_refs(
            owner.workspace.id, agent_id, member.user_id, None, None, 20,
        ).await.unwrap();
        assert!(without_project.is_empty());
        let outsider = outbox.list_authorized_document_refs(
            owner.workspace.id, agent_id, Uuid::new_v4(), None, Some(project.id), 20,
        ).await.unwrap();
        assert!(outsider.is_empty());

        let direct = outbox.get_authorized_document(
            &repository, owner.workspace.id, document.memory_id, 1, agent_id, member.user_id, None,
        ).await.unwrap();
        assert!(direct.is_some());
        assert!(storage.revoke_project_member(
            owner.workspace.id, project.id, owner.user_id, member.user_id,
        ).await.unwrap());
        let revoked = outbox.get_authorized_document(
            &repository, owner.workspace.id, document.memory_id, 1, agent_id, member.user_id, None,
        ).await.unwrap();
        assert!(revoked.is_none());
    }

    #[tokio::test]
    async fn retrieval_feedback_is_session_pinned_authorized_and_audited() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage.ensure_firebase_user("axon-test", "feedback-owner").await.unwrap();
        let agent = crate::model::Agent {
            id: Uuid::new_v4(),
            workspace_id: account.workspace.id,
            created_by_user_id: Some(account.user_id),
            display_name: "Feedback agent".into(),
            provider: "test".into(),
            connection_mode: crate::model::ConnectionMode::Manual,
            created_at: Utc::now(),
        };
        storage.apply(crate::storage::StorageMutation {
            entity: crate::storage::StoredEntity::Agent(agent.clone()),
            related_entities: Vec::new(),
            audit: crate::audit::AuditEvent::attributed(
                format!("human:{}", account.user_id),
                Some(account.workspace.id),
                "agent.registered",
                "agent",
                agent.id.to_string(),
                serde_json::json!({}),
            ),
        }).await.unwrap();

        let outbox = storage.memory_outbox();
        let repository = FakeDocumentRepository::default();
        let mut document = super::tests::document(account.workspace.id, agent.id);
        document.scope = "workspace_shared".into();
        document.owner_id = None;
        document.status = "proposed".into();
        outbox.enqueue(&mut document, account.user_id).await.unwrap();
        repository.put_version(document.clone()).await.unwrap();
        assert!(outbox.approve(account.workspace.id, document.memory_id, 1, account.user_id).await.unwrap());
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(storage.memory_epoch_coordinator().publish_due_epochs().await.unwrap(), 1);
        let epoch_id: String = sqlx::query_scalar(
            "SELECT id FROM memory_epochs WHERE workspace_id = ? ORDER BY epoch_number DESC LIMIT 1",
        )
        .bind(account.workspace.id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let session_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agent_memory_sessions (session_id, workspace_id, agent_id, task_id, memory_epoch_id, initial_memory_ids_json, retrieved_memory_ids_json, created_at, project_id) VALUES (?, ?, ?, NULL, ?, '[]', '[]', ?, NULL)",
        )
        .bind(session_id.to_string())
        .bind(account.workspace.id.to_string())
        .bind(agent.id.to_string())
        .bind(epoch_id)
        .bind(Utc::now().to_rfc3339())
        .execute(&outbox.pool)
        .await
        .unwrap();

        let recorded = outbox.record_retrieval_feedback(
            account.workspace.id,
            session_id,
            agent.id,
            account.user_id,
            document.memory_id,
            1,
            "helpful",
            &serde_json::json!({"reason": "resolved the task"}),
        ).await.unwrap().unwrap();
        assert_eq!(recorded.event_type, "helpful");
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM memory_retrieval_feedback WHERE session_id = ? AND memory_id = ? AND document_version = 1",
        )
        .bind(session_id.to_string())
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(count, 1);
        let signals = outbox
            .feedback_signals(account.workspace.id, &[(document.memory_id, 1)])
            .await
            .unwrap();
        let (positive, negative) = signals.get(&(document.memory_id, 1)).copied().unwrap();
        assert!((positive - (2.0 / 3.0)).abs() < 1e-6);
        assert!((negative - (1.0 / 3.0)).abs() < 1e-6);

        let invalid = outbox.record_retrieval_feedback(
            account.workspace.id, session_id, agent.id, account.user_id,
            document.memory_id, 1, "made_up_event", &serde_json::json!({}),
        ).await;
        assert!(matches!(invalid, Err(MemoryOutboxError::VersionConflict)));
        let unpinned = outbox.record_retrieval_feedback(
            account.workspace.id, session_id, agent.id, account.user_id,
            Uuid::new_v4(), 1, "helpful", &serde_json::json!({}),
        ).await.unwrap();
        assert!(unpinned.is_none());
    }

    #[tokio::test]
    async fn archive_restore_and_delete_create_new_versions_and_control_current_retrieval() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage.ensure_firebase_user("axon-test", "lifecycle-user").await.unwrap();
        let outbox = storage.memory_outbox();
        let repository = FakeDocumentRepository::default();
        let mut document = super::tests::document(account.workspace.id, Uuid::new_v4());
        document.scope = "workspace_shared".into();
        document.owner_id = None;
        document.status = "proposed".into();
        document.validate().unwrap();
        outbox.enqueue(&mut document, account.user_id).await.unwrap();
        repository.put_version(document.clone()).await.unwrap();
        assert!(outbox.approve(account.workspace.id, document.memory_id, 1, account.user_id).await.unwrap());
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        assert!(outbox.transition_lifecycle(
            &repository, account.workspace.id, document.memory_id, 1, account.user_id,
            MemoryLifecycleAction::Archive,
        ).await.unwrap());
        let archived_payload: String = sqlx::query_scalar(
            "SELECT payload_json FROM memory_document_outbox WHERE memory_id = ? AND version = 2",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let archived: MemoryDocument = serde_json::from_str(&archived_payload).unwrap();
        repository.put_version(archived).await.unwrap();
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 2",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();
        let hidden = outbox.list_authorized_document_refs(
            account.workspace.id, Uuid::nil(), account.user_id, None, None, 20,
        ).await.unwrap();
        assert!(hidden.is_empty());
        let coordinator = storage.memory_epoch_coordinator();
        assert_eq!(coordinator.publish_due_epochs().await.unwrap(), 1);
        let archived_epoch_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM memory_document_epoch_items WHERE workspace_id = ?",
        )
        .bind(account.workspace.id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(archived_epoch_count, 0);
        let direct_archived = outbox.get_authorized_document(
            &repository, account.workspace.id, document.memory_id, 1, Uuid::nil(), account.user_id, None,
        ).await.unwrap();
        assert!(direct_archived.is_none());

        assert!(outbox.transition_lifecycle(
            &repository, account.workspace.id, document.memory_id, 2, account.user_id,
            MemoryLifecycleAction::Restore,
        ).await.unwrap());
        let restored_payload: String = sqlx::query_scalar(
            "SELECT payload_json FROM memory_document_outbox WHERE memory_id = ? AND version = 3",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let restored: MemoryDocument = serde_json::from_str(&restored_payload).unwrap();
        assert_eq!(restored.status, "ready");
        repository.put_version(restored).await.unwrap();
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 3",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();
        let visible = outbox.list_authorized_document_refs(
            account.workspace.id, Uuid::nil(), account.user_id, None, None, 20,
        ).await.unwrap();
        assert_eq!(visible, vec![(document.memory_id, 3)]);
        assert_eq!(coordinator.publish_due_epochs().await.unwrap(), 1);
        let restored_epoch_version: i64 = sqlx::query_scalar(
            "SELECT i.document_version FROM memory_document_epoch_items i
             JOIN memory_epochs e ON e.id = i.epoch_id
             WHERE i.workspace_id = ? AND i.memory_id = ?
             ORDER BY e.epoch_number DESC LIMIT 1",
        )
        .bind(account.workspace.id.to_string())
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(restored_epoch_version, 3);
        let direct_restored = outbox.get_authorized_document(
            &repository, account.workspace.id, document.memory_id, 3, Uuid::nil(), account.user_id, None,
        ).await.unwrap();
        assert!(direct_restored.is_some());

        assert!(outbox.transition_lifecycle(
            &repository, account.workspace.id, document.memory_id, 3, account.user_id,
            MemoryLifecycleAction::Delete,
        ).await.unwrap());
        let deleted_status: String = sqlx::query_scalar(
            "SELECT lifecycle_status FROM memory_document_versions WHERE memory_id = ? AND version = 4",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(deleted_status, "deleted");
        let hidden_after_delete = outbox.list_authorized_document_refs(
            account.workspace.id, Uuid::nil(), account.user_id, None, None, 20,
        ).await.unwrap();
        assert!(hidden_after_delete.is_empty());
        let direct_deleted = outbox.get_authorized_document(
            &repository, account.workspace.id, document.memory_id, 3, Uuid::nil(), account.user_id, None,
        ).await.unwrap();
        assert!(direct_deleted.is_none());
    }

    #[tokio::test]
    async fn epoch_hydration_returns_only_the_pinned_authorized_document_version() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let outbox = storage.memory_outbox();
        let workspace_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();
        let mut document = super::tests::document(workspace_id, agent_id);
        outbox.enqueue(&mut document, Uuid::new_v4()).await.unwrap();

        let repository = FakeDocumentRepository::default();
        repository.put_version(document.clone()).await.unwrap();
        assert!(outbox.approve(workspace_id, document.memory_id, 1, Uuid::new_v4()).await.unwrap());
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        let coordinator = storage
            .memory_epoch_coordinator()
            .with_qoin_scoring_version("test-qoin-memory-v2".into());
        assert_eq!(coordinator.publish_due_epochs().await.unwrap(), 1);
        let epoch_id_text: String = sqlx::query_scalar(
            "SELECT id FROM memory_epochs WHERE workspace_id = ? ORDER BY epoch_number DESC LIMIT 1",
        )
        .bind(workspace_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let epoch_id = Uuid::parse_str(&epoch_id_text).unwrap();
        let pinned_scoring_version: String = sqlx::query_scalar(
            "SELECT qoin_scoring_version FROM memory_document_epoch_items WHERE epoch_id = ? LIMIT 1",
        )
        .bind(epoch_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(pinned_scoring_version, "test-qoin-memory-v2");

        let visible = outbox.list_epoch_authorized_documents(
            &repository, workspace_id, epoch_id, agent_id, Uuid::nil(), None, 20,
        ).await.unwrap().unwrap();
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].memory_id, document.memory_id);
        assert_eq!(visible[0].version, 1);
        assert_eq!(visible[0].content_hash, document.content_hash);
        assert_eq!(visible[0].status, "ready");

        let hidden = outbox.list_epoch_authorized_documents(
            &repository, workspace_id, epoch_id, Uuid::new_v4(), Uuid::nil(), None, 20,
        ).await.unwrap().unwrap();
        assert!(hidden.is_empty());

        let session_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO agent_memory_sessions (session_id, workspace_id, agent_id, task_id, memory_epoch_id, initial_memory_ids_json, retrieved_memory_ids_json, created_at, project_id) VALUES (?, ?, ?, NULL, ?, '[]', '[]', ?, NULL)",
        )
        .bind(session_id.to_string())
        .bind(workspace_id.to_string())
        .bind(agent_id.to_string())
        .bind(epoch_id.to_string())
        .bind(Utc::now().to_rfc3339())
        .execute(&outbox.pool)
        .await
        .unwrap();
        let session_documents = outbox.list_session_authorized_documents(
            &repository, workspace_id, session_id, agent_id, Uuid::nil(), 20,
        ).await.unwrap().unwrap();
        assert_eq!(session_documents.0, epoch_id);
        assert!(session_documents.1.is_none());
        assert_eq!(session_documents.2.as_deref(), Some("test-qoin-memory-v2"));
        assert_eq!(session_documents.3.len(), 1);
        assert_eq!(session_documents.3[0].memory_id, document.memory_id);

        let denied_session = outbox.list_session_authorized_documents(
            &repository, workspace_id, session_id, Uuid::new_v4(), Uuid::nil(), 20,
        ).await.unwrap();
        assert!(denied_session.is_none());

        let missing = outbox.list_epoch_authorized_documents(
            &repository, workspace_id, Uuid::new_v4(), agent_id, Uuid::nil(), None, 20,
        ).await.unwrap();
        assert!(missing.is_none());
    }
}

#[cfg(test)]
mod outbox_recovery_tests {
    use super::*;
    use crate::storage::SqliteStorage;

    #[tokio::test]
    async fn dead_letter_recovery_requeues_event_and_audits_atomically() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let outbox = storage.memory_outbox();
        let workspace_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();
        let actor_id = Uuid::new_v4();
        let mut document = super::tests::document(workspace_id, agent_id);
        outbox.enqueue(&mut document, actor_id).await.unwrap();

        sqlx::query(
            "UPDATE memory_document_outbox SET status = 'dead_letter', attempts = 8 WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'failed' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        assert!(outbox.retry_dead_letter(
            workspace_id, document.memory_id, 1, actor_id,
        ).await.unwrap());

        let status: String = sqlx::query_scalar(
            "SELECT status FROM memory_document_outbox WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let attempts: i64 = sqlx::query_scalar(
            "SELECT attempts FROM memory_document_outbox WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let sync_status: String = sqlx::query_scalar(
            "SELECT sync_status FROM memory_document_versions WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        let audit_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM audit_events WHERE event_type = 'memory_document.outbox_requeued' AND entity_id = ?",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();

        assert_eq!(status, "pending");
        assert_eq!(attempts, 0);
        assert_eq!(sync_status, "pending");
        assert_eq!(audit_count, 1);
    }

    #[tokio::test]
    async fn malformed_dead_letter_payload_cannot_be_requeued() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let outbox = storage.memory_outbox();
        let workspace_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();
        let mut document = super::tests::document(workspace_id, agent_id);
        outbox.enqueue(&mut document, Uuid::new_v4()).await.unwrap();

        sqlx::query(
            "UPDATE memory_document_outbox SET status = 'dead_letter', payload_json = 'not-json' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'failed' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        assert!(!outbox.retry_dead_letter(
            workspace_id, document.memory_id, 1, Uuid::new_v4(),
        ).await.unwrap());
    }
}

#[cfg(test)]
mod reconciliation_tests {
    use super::*;
    use crate::storage::SqliteStorage;
    use std::{collections::HashMap, sync::Mutex};

    #[derive(Default)]
    struct Repository {
        documents: Mutex<HashMap<(Uuid, u64), MemoryDocument>>,
    }

    #[async_trait::async_trait]
    impl MemoryDocumentRepository for Repository {
        async fn put_version(&self, document: MemoryDocument) -> Result<(), MemoryDocumentError> {
            document.validate()?;
            self.documents.lock().map_err(|_| MemoryDocumentError::InvalidDocument)?
                .insert((document.memory_id, document.version), document);
            Ok(())
        }

        async fn get_version(&self, memory_id: Uuid, version: u64)
            -> Result<Option<MemoryDocument>, MemoryDocumentError> {
            Ok(self.documents.lock().map_err(|_| MemoryDocumentError::InvalidDocument)?
                .get(&(memory_id, version)).cloned())
        }

        async fn list_versions(&self, memory_id: Uuid)
            -> Result<Vec<MemoryDocument>, MemoryDocumentError> {
            let mut values = self.documents.lock().map_err(|_| MemoryDocumentError::InvalidDocument)?
                .values().filter(|item| item.memory_id == memory_id).cloned().collect::<Vec<_>>();
            values.sort_by_key(|item| item.version);
            Ok(values)
        }

        async fn list_workspace_documents(&self, workspace_id: Uuid, limit: usize)
            -> Result<Vec<MemoryDocument>, MemoryDocumentError> {
            let mut values = self.documents.lock().map_err(|_| MemoryDocumentError::InvalidDocument)?
                .values().filter(|item| item.workspace_id == workspace_id).cloned().collect::<Vec<_>>();
            values.sort_by(|left, right| left.memory_id.as_bytes().cmp(right.memory_id.as_bytes())
                .then_with(|| left.version.cmp(&right.version)));
            values.truncate(limit.clamp(1, 5_000));
            Ok(values)
        }

        async fn verify_version(&self, memory_id: Uuid, version: u64, expected_hash: &str)
            -> Result<bool, MemoryDocumentError> {
            Ok(self.get_version(memory_id, version).await?
                .is_some_and(|item| item.content_hash == expected_hash))
        }

        async fn ping(&self) -> Result<(), MemoryDocumentError> { Ok(()) }
    }

    #[tokio::test]
    async fn reconciliation_detects_pending_missing_and_metadata_mismatch() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let outbox = storage.memory_outbox();
        let workspace_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();
        let mut document = super::tests::document(workspace_id, agent_id);
        outbox.enqueue(&mut document, Uuid::new_v4()).await.unwrap();

        let repository = Repository::default();
        let pending = outbox.reconcile_workspace(&repository, workspace_id, 100).await.unwrap();
        assert_eq!(pending.pending_outbox_events, 1);
        assert!(pending.issues.iter().any(|issue|
            issue.kind == "mongo_document_pending_delivery"));

        let mut conflicting = document.clone();
        conflicting.scope = "workspace_shared".into();
        conflicting.owner_id = None;
        repository.put_version(conflicting).await.unwrap();
        let mismatch = outbox.reconcile_workspace(&repository, workspace_id, 100).await.unwrap();
        assert!(mismatch.issues.iter().any(|issue|
            issue.kind == "scope_mismatch"));
    }
}

#[cfg(test)]
mod latest_version_search_tests {
    use super::*;
    use crate::storage::SqliteStorage;

    #[tokio::test]
    async fn search_candidate_refs_return_only_latest_approved_synchronized_version() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let outbox = storage.memory_outbox();
        let workspace_id = Uuid::new_v4();
        let agent_id = Uuid::new_v4();
        let actor_id = Uuid::new_v4();
        let mut first = super::tests::document(workspace_id, agent_id);
        outbox.enqueue(&mut first, actor_id).await.unwrap();
        assert!(outbox.approve(workspace_id, first.memory_id, 1, actor_id).await.unwrap());
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 1",
        )
        .bind(first.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        let mut second = first.clone();
        second.version = 2;
        second.source_event_sequence = 0;
        second.title = "Updated private memory".into();
        second.content = "The newer approved version supersedes the old search candidate.".into();
        second.updated_at = Utc::now();
        second.content_hash = second.calculate_content_hash();
        outbox.enqueue(&mut second, actor_id).await.unwrap();
        assert!(outbox.approve(workspace_id, second.memory_id, 2, actor_id).await.unwrap());
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 2",
        )
        .bind(second.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        let refs = outbox.list_authorized_document_refs(
            workspace_id, agent_id, Uuid::nil(), None, None, 200,
        ).await.unwrap();
        assert_eq!(refs, vec![(first.memory_id, 2)]);

        let first_history_page = outbox.list_authorized_document_versions(
            workspace_id, first.memory_id, agent_id, Uuid::nil(), None, None, None, 1,
        ).await.unwrap();
        assert_eq!(first_history_page.iter().map(|item| item.version).collect::<Vec<_>>(), vec![2]);
        let second_history_page = outbox.list_authorized_document_versions(
            workspace_id, first.memory_id, agent_id, Uuid::nil(), None, None,
            first_history_page.last().map(|item| item.version), 1,
        ).await.unwrap();
        assert_eq!(second_history_page.iter().map(|item| item.version).collect::<Vec<_>>(), vec![1]);
        let denied_history = outbox.list_authorized_document_versions(
            workspace_id, first.memory_id, Uuid::new_v4(), Uuid::nil(), None, None, None, 100,
        ).await.unwrap();
        assert!(denied_history.is_empty());

        let coordinator = storage.memory_epoch_coordinator();
        assert_eq!(coordinator.publish_due_epochs().await.unwrap(), 1);
        let pinned_versions: Vec<i64> = sqlx::query_scalar(
            "SELECT document_version FROM memory_document_epoch_items WHERE memory_id = ? ORDER BY document_version",
        )
        .bind(first.memory_id.to_string())
        .fetch_all(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(pinned_versions, vec![2]);
    }
}
