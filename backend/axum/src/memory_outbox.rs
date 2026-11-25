//! Durable SQLite outbox coordinating authoritative memory metadata with MongoDB.
//!
//! A committed outbox row is recoverable after process restart. The worker uses
//! leases and immutable (memory_id, version) keys so duplicate delivery is safe.

use crate::memory_documents::{
    MemoryDocument, MemoryDocumentError, MemoryDocumentRepository,
};
use chrono::{Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Row, SqlitePool};
use std::{sync::Arc, time::Duration};
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

#[derive(Clone)]
pub struct MemoryOutbox {
    pool: SqlitePool,
}

impl MemoryOutbox {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Atomically records authoritative metadata and its MongoDB write event.
    /// Repeating the same memory/version/hash is idempotent.
    pub async fn enqueue(&self, document: &MemoryDocument) -> Result<(), MemoryOutboxError> {
        document
            .validate()
            .map_err(|error| sqlx::Error::Protocol(error.to_string()))?;
        let payload = serde_json::to_string(document)?;
        let now = Utc::now().to_rfc3339();
        let lifecycle_status = match document.status.as_str() {
            "proposed" => "proposed",
            "archived" => "archived",
            "ready" => "approved",
            _ => return Err(MemoryOutboxError::VersionConflict),
        };
        let version = i64::try_from(document.version)
            .map_err(|_| sqlx::Error::Protocol("memory version exceeds SQLite range".into()))?;
        let source_sequence = i64::try_from(document.source_event_sequence)
            .map_err(|_| sqlx::Error::Protocol("source sequence exceeds SQLite range".into()))?;

        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO memory_document_versions              (memory_id, version, workspace_id, project_id, owner_id, task_id, scope,               document_type, content_hash, source_event_sequence, qkv_version,               qoin_vector_version, lifecycle_status, sync_status, created_at, updated_at)              VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'pending', ?, ?)              ON CONFLICT(memory_id, version) DO NOTHING",
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
        .bind(source_sequence)
        .bind(&document.qkv_version)
        .bind(&document.qoin_vector_version)
        .bind(lifecycle_status)
        .bind(&now)
        .bind(&now)
        .execute(&mut *transaction)
        .await?;

        let existing = sqlx::query(
            "SELECT workspace_id, project_id, owner_id, task_id, scope, document_type, \
             content_hash, source_event_sequence, qkv_version, qoin_vector_version, lifecycle_status \
             FROM memory_document_versions WHERE memory_id = ? AND version = ?",
        )
        .bind(document.memory_id.to_string())
        .bind(version)
        .fetch_one(&mut *transaction)
        .await?;
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
        if stored_workspace != document.workspace_id.to_string()
            || stored_project != document.project_id.map(|id| id.to_string())
            || stored_owner != document.owner_id.map(|id| id.to_string())
            || stored_task != document.task_id.map(|id| id.to_string())
            || stored_scope != document.scope
            || stored_type != document.document_type
            || stored_hash != document.content_hash
            || stored_sequence != source_sequence
            || stored_qkv_version != document.qkv_version
            || stored_qoin_version != document.qoin_vector_version
            || stored_lifecycle != lifecycle_status
        {
            return Err(MemoryOutboxError::VersionConflict);
        }

        let sync_status: String = sqlx::query_scalar(
            "SELECT sync_status FROM memory_document_versions WHERE memory_id = ? AND version = ?",
        )
        .bind(document.memory_id.to_string())
        .bind(version)
        .fetch_one(&mut *transaction)
        .await?;
        if sync_status == "pending" || sync_status == "failed" {
            sqlx::query(
                "INSERT INTO memory_document_outbox                  (event_id, memory_id, version, event_type, payload_json, status, attempts,                   available_at, created_at, updated_at)                  VALUES (?, ?, ?, 'memory_document.put', ?, 'pending', 0, ?, ?, ?)                  ON CONFLICT(memory_id, version, event_type) DO NOTHING",
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
        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored', updated_at = ?              WHERE memory_id = ? AND version = ? AND content_hash = ?",
        )
        .bind(&now)
        .bind(event.document.memory_id.to_string())
        .bind(version)
        .bind(&event.document.content_hash)
        .execute(&mut *transaction)
        .await?;
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
        sqlx::query(
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

    /// Lists only approved, synchronized document versions visible to an
    /// agent/task pair. Project-scoped documents remain excluded until project
    /// membership authorization is available.
    pub async fn list_authorized_document_refs(
        &self,
        workspace_id: Uuid,
        requester_agent_id: Uuid,
        task_id: Option<Uuid>,
        limit: usize,
    ) -> Result<Vec<(Uuid, u64)>, MemoryOutboxError> {
        let task_id_text = task_id.map(|id| id.to_string());
        let limit = i64::try_from(limit.clamp(1, 200))
            .map_err(|_| sqlx::Error::Protocol("retrieval limit exceeds SQLite range".into()))?;
        let rows = sqlx::query(
            "SELECT memory_id, version FROM memory_document_versions \
             WHERE workspace_id = ? AND lifecycle_status = 'approved' AND sync_status IN ('stored', 'indexed') \
             AND (scope = 'workspace_shared' OR (scope = 'agent_private' AND owner_id = ?) \
             OR (scope = 'task_shared' AND task_id = ?)) \
             ORDER BY updated_at DESC, memory_id ASC, version DESC LIMIT ?",
        )
        .bind(workspace_id.to_string())
        .bind(requester_agent_id.to_string())
        .bind(task_id_text)
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
        task_id: Option<Uuid>,
    ) -> Result<Option<MemoryDocument>, MemoryOutboxError> {
        let version_sql = i64::try_from(version)
            .map_err(|_| sqlx::Error::Protocol("memory version exceeds SQLite range".into()))?;
        let row = sqlx::query(
            "SELECT scope, owner_id, task_id, content_hash, lifecycle_status, sync_status \
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
        let expected_hash: String = row.try_get("content_hash")?;
        let lifecycle_status: String = row.try_get("lifecycle_status")?;
        let sync_status: String = row.try_get("sync_status")?;
        if lifecycle_status != "approved" || !matches!(sync_status.as_str(), "stored" | "indexed") {
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
            // Project membership is not persisted yet, so fail closed.
            "project_shared" => false,
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
    use crate::storage::SqliteStorage;

    fn document(workspace_id: Uuid, owner_agent_id: Uuid) -> MemoryDocument {
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
            source_event_sequence: 1,
            created_at: now,
            updated_at: now,
            content_hash: String::new(),
            key_metadata: serde_json::json!({}),
            value_payload: serde_json::json!({}),
            qkv_version: "qkv-metadata-v1".into(),
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
        let document = document(workspace_id, agent_id);

        outbox.enqueue(&document).await.unwrap();
        outbox.enqueue(&document).await.unwrap();

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
            "SELECT COUNT(*) FROM audit_events WHERE event_type = 'memory_document.approved' AND entity_id = ?",
        )
        .bind(document.memory_id.to_string())
        .fetch_one(&outbox.pool)
        .await
        .unwrap();
        assert_eq!(audit_count, 1);

        sqlx::query(
            "UPDATE memory_document_versions SET sync_status = 'stored' WHERE memory_id = ? AND version = 1",
        )
        .bind(document.memory_id.to_string())
        .execute(&outbox.pool)
        .await
        .unwrap();

        let visible = outbox
            .list_authorized_document_refs(workspace_id, agent_id, None, 20)
            .await
            .unwrap();
        let hidden = outbox
            .list_authorized_document_refs(workspace_id, Uuid::new_v4(), None, 20)
            .await
            .unwrap();
        assert_eq!(visible, vec![(document.memory_id, 1)]);
        assert!(hidden.is_empty());
    }
}
