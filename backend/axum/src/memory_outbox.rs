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

        let stored_hash: String = sqlx::query_scalar(
            "SELECT content_hash FROM memory_document_versions WHERE memory_id = ? AND version = ?",
        )
        .bind(document.memory_id.to_string())
        .bind(version)
        .fetch_one(&mut *transaction)
        .await?;
        if stored_hash != document.content_hash {
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
