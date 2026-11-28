//! SQLite persistence for scoped agent memory and allocation audit records.
use crate::memory_system::{
    allocate_context, lexical_relevance, MemoryAllocation, MemoryError, MemoryRecord,
    RetrievalContext,
};
use chrono::Utc;
use serde_json::json;
use sqlx::{Row, SqlitePool};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum MemoryStoreError {
    #[error("database operation failed: {0}")]
    Database(#[from] sqlx::Error),
    #[error("memory serialization failed: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error(transparent)]
    Policy(#[from] MemoryError),
    #[error("memory record was not found in this workspace")]
    NotFound,
}

#[derive(Clone)]
pub struct MemoryStore {
    pool: SqlitePool,
}

fn scope_label(value: crate::memory_system::MemoryScope) -> &'static str {
    match value {
        crate::memory_system::MemoryScope::Workspace => "workspace",
        crate::memory_system::MemoryScope::Task => "task",
        crate::memory_system::MemoryScope::Agent => "agent",
        crate::memory_system::MemoryScope::Execution => "execution",
    }
}

fn sensitivity_label(value: crate::memory_system::MemorySensitivity) -> &'static str {
    match value {
        crate::memory_system::MemorySensitivity::Public => "public",
        crate::memory_system::MemorySensitivity::Internal => "internal",
        crate::memory_system::MemorySensitivity::Confidential => "confidential",
        crate::memory_system::MemorySensitivity::Secret => "secret",
    }
}

fn verification_label(value: crate::memory_system::Verification) -> &'static str {
    match value {
        crate::memory_system::Verification::Unverified => "unverified",
        crate::memory_system::Verification::Proposed => "proposed",
        crate::memory_system::Verification::Verified => "verified",
        crate::memory_system::Verification::Rejected => "rejected",
    }
}

impl MemoryStore {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Save a new proposal or a verified memory revision. The complete
    /// previous version is retained for provenance and conflict inspection.
    pub async fn put(&self, record: MemoryRecord, actor: &str) -> Result<(), MemoryStoreError> {
        record.validate()?;
        let payload = serde_json::to_string(&record)?;
        let now = Utc::now().to_rfc3339();
        let mut tx = self.pool.begin().await?;
        let existing_row =
            sqlx::query("SELECT workspace_id, version FROM agent_memories WHERE id = ?")
                .bind(record.id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
        if let Some(row) = &existing_row {
            let existing_workspace: String = row.try_get("workspace_id")?;
            let existing_version: i64 = row.try_get("version")?;
            if existing_workspace != record.workspace_id.to_string() {
                return Err(MemoryStoreError::NotFound);
            }
            if existing_version as u64 + 1 != record.version {
                return Err(MemoryStoreError::Policy(MemoryError::InvalidRecord));
            }
        }
        let existing = existing_row.is_some();
        if existing && record.version == 1 {
            return Err(MemoryStoreError::Policy(MemoryError::InvalidRecord));
        }
        sqlx::query("INSERT INTO agent_memories (id, workspace_id, scope, scope_id, owner_agent_id, sensitivity, verification, content, source, confidence, authority, created_at, expires_at, version, token_cost, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET scope=excluded.scope, scope_id=excluded.scope_id, owner_agent_id=excluded.owner_agent_id, sensitivity=excluded.sensitivity, verification=excluded.verification, content=excluded.content, source=excluded.source, confidence=excluded.confidence, authority=excluded.authority, expires_at=excluded.expires_at, version=excluded.version, token_cost=excluded.token_cost, updated_at=excluded.updated_at WHERE agent_memories.workspace_id=excluded.workspace_id")
            .bind(record.id.to_string())
            .bind(record.workspace_id.to_string())
            .bind(scope_label(record.scope))
            .bind(record.scope_id.map(|id| id.to_string()))
            .bind(record.owner_agent_id.map(|id| id.to_string()))
            .bind(sensitivity_label(record.sensitivity))
            .bind(verification_label(record.verification))
            .bind(&record.content)
            .bind(&record.source)
            .bind(record.confidence)
            .bind(record.authority)
            .bind(record.created_at.to_rfc3339())
            .bind(record.expires_at.map(|value| value.to_rfc3339()))
            .bind(record.version as i64)
            .bind(record.token_cost as i64)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO agent_memory_versions (memory_id, version, payload_json, actor, event_type, occurred_at) VALUES (?, ?, ?, ?, ?, ?)")
            .bind(record.id.to_string())
            .bind(record.version as i64)
            .bind(payload)
            .bind(actor)
            .bind(if !existing {
                "proposed"
            } else {
                match record.verification {
                    crate::memory_system::Verification::Verified => "approved",
                    crate::memory_system::Verification::Rejected => "rejected",
                    _ => "revised",
                }
            })
            .bind(now)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, ?, 'agent_memory', ?, ?, ?)")
            .bind(Uuid::new_v4().to_string())
            .bind(record.workspace_id.to_string())
            .bind(actor)
            .bind(if !existing {
                "agent_memory.proposed"
            } else {
                match record.verification {
                    crate::memory_system::Verification::Verified => "agent_memory.approved",
                    crate::memory_system::Verification::Rejected => "agent_memory.rejected",
                    _ => "agent_memory.revised",
                }
            })
            .bind(record.id.to_string())
            .bind(Utc::now().to_rfc3339())
            .bind(
                json!({
                    "scope": record.scope,
                    "verification": record.verification,
                    "version": record.version
                })
                .to_string(),
            )
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Grant an agent read access to a private memory record. A grant is
    /// workspace-bound and revocation is represented by a timestamp.
    pub async fn grant(
        &self,
        workspace_id: Uuid,
        memory_id: Uuid,
        agent_id: Uuid,
        actor: &str,
    ) -> Result<(), MemoryStoreError> {
        let now = Utc::now().to_rfc3339();
        let mut tx = self.pool.begin().await?;
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM agent_memories WHERE id = ? AND workspace_id = ? AND verification = 'verified'",
        )
        .bind(memory_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if exists.is_none() {
            return Err(MemoryStoreError::NotFound);
        }
        sqlx::query("INSERT INTO agent_memory_grants (memory_id, agent_id, granted_by, granted_at, revoked_at) VALUES (?, ?, ?, ?, NULL) ON CONFLICT(memory_id, agent_id) DO UPDATE SET granted_by=excluded.granted_by, granted_at=excluded.granted_at, revoked_at=NULL")
            .bind(memory_id.to_string())
            .bind(agent_id.to_string())
            .bind(actor)
            .bind(&now)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'agent_memory.shared', 'agent_memory', ?, ?, ?)")
            .bind(Uuid::new_v4().to_string())
            .bind(workspace_id.to_string())
            .bind(actor)
            .bind(memory_id.to_string())
            .bind(now)
            .bind(json!({"agent_id": agent_id}).to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    pub async fn revoke(
        &self,
        workspace_id: Uuid,
        memory_id: Uuid,
        agent_id: Uuid,
        actor: &str,
    ) -> Result<bool, MemoryStoreError> {
        let now = Utc::now().to_rfc3339();
        let mut tx = self.pool.begin().await?;
        let exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM agent_memories WHERE id = ? AND workspace_id = ?")
                .bind(memory_id.to_string())
                .bind(workspace_id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
        if exists.is_none() {
            return Err(MemoryStoreError::NotFound);
        }
        let result = sqlx::query("UPDATE agent_memory_grants SET revoked_at = ? WHERE memory_id = ? AND agent_id = ? AND revoked_at IS NULL")
            .bind(&now)
            .bind(memory_id.to_string())
            .bind(agent_id.to_string())
            .execute(&mut *tx)
            .await?;
        if result.rows_affected() > 0 {
            sqlx::query("INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'agent_memory.share_revoked', 'agent_memory', ?, ?, ?)")
                .bind(Uuid::new_v4().to_string())
                .bind(workspace_id.to_string())
                .bind(actor)
                .bind(memory_id.to_string())
                .bind(now)
                .bind(json!({"agent_id": agent_id}).to_string())
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await?;
        Ok(result.rows_affected() > 0)
    }

    /// Human review promotes a proposal or explicitly rejects it. Every
    /// decision creates a new immutable version; conflicts are never erased.
    pub async fn review(
        &self,
        workspace_id: Uuid,
        memory_id: Uuid,
        approved: bool,
        actor: &str,
    ) -> Result<MemoryRecord, MemoryStoreError> {
        let row = sqlx::query("SELECT v.payload_json FROM agent_memory_versions v JOIN agent_memories m ON m.id=v.memory_id WHERE m.id=? AND m.workspace_id=? ORDER BY v.version DESC LIMIT 1")
            .bind(memory_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_optional(&self.pool)
            .await?
            .ok_or(MemoryStoreError::NotFound)?;
        let payload: String = row.try_get("payload_json")?;
        let mut record: MemoryRecord = serde_json::from_str(&payload)?;
        if matches!(
            record.verification,
            crate::memory_system::Verification::Verified
                | crate::memory_system::Verification::Rejected
        ) {
            return Err(MemoryStoreError::Policy(MemoryError::InvalidRecord));
        }
        record.verification = if approved {
            crate::memory_system::Verification::Verified
        } else {
            crate::memory_system::Verification::Rejected
        };
        record.version = record
            .version
            .checked_add(1)
            .ok_or(MemoryStoreError::Policy(MemoryError::InvalidRecord))?;
        self.put(record.clone(), actor).await?;
        Ok(record)
    }

    /// Permanently remove expired content and its versions before retrieval.
    /// The append-only workspace audit receives a content-free tombstone.
    pub async fn purge_expired(
        &self,
        workspace_id: Uuid,
        now: chrono::DateTime<Utc>,
    ) -> Result<u64, MemoryStoreError> {
        let mut tx = self.pool.begin().await?;
        let rows = sqlx::query("SELECT id FROM agent_memories WHERE workspace_id = ? AND expires_at IS NOT NULL AND expires_at <= ?")
            .bind(workspace_id.to_string())
            .bind(now.to_rfc3339())
            .fetch_all(&mut *tx)
            .await?;
        let mut removed = 0u64;
        for row in rows {
            let id: String = row.try_get("id")?;
            sqlx::query("DELETE FROM agent_memory_grants WHERE memory_id = ?")
                .bind(&id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM agent_memory_versions WHERE memory_id = ?")
                .bind(&id)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM agent_memories WHERE id = ? AND workspace_id = ?")
                .bind(&id)
                .bind(workspace_id.to_string())
                .execute(&mut *tx)
                .await?;
            sqlx::query("INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, 'system:memory-ttl', 'agent_memory.expired', 'agent_memory', ?, ?, '{}')")
                .bind(Uuid::new_v4().to_string())
                .bind(workspace_id.to_string())
                .bind(&id)
                .bind(now.to_rfc3339())
                .execute(&mut *tx)
                .await?;
            removed += 1;
        }
        tx.commit().await?;
        Ok(removed)
    }

    /// Retrieve and rank only records visible to the requesting agent, then
    /// persist the exact context allocation for later inspection.
    pub async fn allocate(
        &self,
        context: &RetrievalContext,
        query: &str,
        token_budget: usize,
    ) -> Result<MemoryAllocation, MemoryStoreError> {
        self.purge_expired(context.workspace_id, context.now)
            .await?;
        let rows = sqlx::query("SELECT v.payload_json FROM agent_memory_versions v JOIN (SELECT memory_id, MAX(version) AS version FROM agent_memory_versions GROUP BY memory_id) latest ON latest.memory_id=v.memory_id AND latest.version=v.version JOIN agent_memories m ON m.id=v.memory_id WHERE m.workspace_id = ? AND (m.expires_at IS NULL OR m.expires_at > ?)")
            .bind(context.workspace_id.to_string())
            .bind(context.now.to_rfc3339())
            .fetch_all(&self.pool)
            .await?;
        let mut records = Vec::with_capacity(rows.len());
        for row in rows {
            let payload: String = row.try_get("payload_json")?;
            records.push(serde_json::from_str::<MemoryRecord>(&payload)?);
        }
        let grants = sqlx::query(
            "SELECT memory_id FROM agent_memory_grants WHERE agent_id = ? AND revoked_at IS NULL",
        )
        .bind(context.agent_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        let mut scoped_context = context.clone();
        for row in grants {
            let id: String = row.try_get("memory_id")?;
            if let Ok(id) = Uuid::parse_str(&id) {
                scoped_context.granted_memory_ids.insert(id);
            }
        }
        let relevance = records
            .iter()
            .map(|record| (record.id, lexical_relevance(query, &record.content)))
            .collect::<Vec<_>>();
        let allocation = allocate_context(&records, &scoped_context, &relevance, token_budget)?;
        let selected_audit = allocation
            .selected
            .iter()
            .map(|item| {
                json!({
                    "memory_id": item.memory_id,
                    "token_cost": item.token_cost,
                    "utility": item.utility,
                    "rank": item.rank
                })
            })
            .collect::<Vec<_>>();
        let allocation_id = Uuid::new_v4();
        let occurred_at = Utc::now().to_rfc3339();
        let mut tx = self.pool.begin().await?;
        sqlx::query("INSERT INTO agent_memory_allocations (id, workspace_id, task_id, agent_id, token_budget, tokens_allocated, selected_json, excluded_json, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(allocation_id.to_string())
            .bind(allocation.workspace_id.to_string())
            .bind(allocation.task_id.map(|id| id.to_string()))
            .bind(allocation.agent_id.to_string())
            .bind(allocation.token_budget as i64)
            .bind(allocation.tokens_allocated as i64)
            .bind(serde_json::to_string(&selected_audit)?)
            .bind(serde_json::to_string(&allocation.excluded)?)
            .bind(&occurred_at)
            .execute(&mut *tx)
            .await?;
        sqlx::query("INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'agent_memory.allocated', 'agent_memory_allocation', ?, ?, ?)")
            .bind(Uuid::new_v4().to_string())
            .bind(allocation.workspace_id.to_string())
            .bind(format!("agent:{}", allocation.agent_id))
            .bind(allocation_id.to_string())
            .bind(occurred_at)
            .bind(json!({
                "task_id": allocation.task_id,
                "token_budget": allocation.token_budget,
                "tokens_allocated": allocation.tokens_allocated,
                "selected_count": allocation.selected.len(),
                "excluded_count": allocation.excluded.len()
            }).to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(allocation)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        memory_system::{MemoryScope, MemorySensitivity, Verification},
        storage::{SqliteStorage, Storage},
    };
    use chrono::Duration;
    use std::collections::BTreeSet;

    fn agent_memory(workspace_id: Uuid, owner_agent_id: Uuid) -> MemoryRecord {
        MemoryRecord {
            id: Uuid::new_v4(),
            workspace_id,
            scope: MemoryScope::Agent,
            scope_id: None,
            owner_agent_id: Some(owner_agent_id),
            content: "The worker uses a deterministic evidence-first process.".into(),
            source: "human:reviewed-note".into(),
            confidence: 0.95,
            authority: 0.9,
            sensitivity: MemorySensitivity::Internal,
            verification: Verification::Verified,
            created_at: Utc::now(),
            expires_at: Some(Utc::now() + Duration::days(30)),
            version: 1,
            token_cost: 64,
        }
    }

    #[tokio::test]
    async fn private_agent_memory_requires_an_explicit_grant_and_returns_context() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage
            .ensure_firebase_user("memory-test", "memory-store-user")
            .await
            .unwrap();
        let chat_store = storage.chat_memory_store();
        let store = MemoryStore::new(chat_store.sqlite_pool());
        let owner = Uuid::new_v4();
        let recipient = Uuid::new_v4();
        let record = agent_memory(account.workspace.id, owner);
        store.put(record.clone(), "human:test").await.unwrap();

        let context = RetrievalContext {
            workspace_id: account.workspace.id,
            task_id: None,
            agent_id: recipient,
            allowed_sensitivity: MemorySensitivity::Internal,
            granted_memory_ids: BTreeSet::new(),
            now: Utc::now(),
        };
        let hidden = store
            .allocate(&context, "deterministic evidence-first process", 128)
            .await
            .unwrap();
        assert!(hidden.selected.is_empty());

        store
            .grant(account.workspace.id, record.id, recipient, "human:test")
            .await
            .unwrap();
        let visible = store
            .allocate(&context, "deterministic evidence-first process", 128)
            .await
            .unwrap();
        assert_eq!(visible.selected.len(), 1);
        assert_eq!(visible.selected[0].content, record.content);
        assert!(visible.tokens_allocated <= visible.token_budget);
    }

    #[tokio::test]
    async fn review_promotes_a_proposal_as_a_new_immutable_version() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage
            .ensure_firebase_user("memory-test", "memory-store-review-user")
            .await
            .unwrap();
        let store = MemoryStore::new(storage.chat_memory_store().sqlite_pool());
        let mut record = agent_memory(account.workspace.id, Uuid::new_v4());
        record.verification = Verification::Proposed;
        store.put(record.clone(), "human:proposer").await.unwrap();

        let reviewed = store
            .review(account.workspace.id, record.id, true, "human:reviewer")
            .await
            .unwrap();
        assert_eq!(reviewed.version, 2);
        assert_eq!(reviewed.verification, Verification::Verified);
        assert!(matches!(
            store
                .review(account.workspace.id, record.id, false, "human:reviewer")
                .await,
            Err(MemoryStoreError::Policy(_))
        ));
    }

    #[tokio::test]
    async fn expired_memory_is_purged_before_it_can_be_allocated() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage
            .ensure_firebase_user("memory-test", "memory-store-expiry-user")
            .await
            .unwrap();
        let store = MemoryStore::new(storage.chat_memory_store().sqlite_pool());
        let agent_id = Uuid::new_v4();
        let mut record = agent_memory(account.workspace.id, agent_id);
        record.expires_at = Some(Utc::now() - Duration::seconds(1));
        store.put(record.clone(), "human:test").await.unwrap();

        let context = RetrievalContext {
            workspace_id: account.workspace.id,
            task_id: None,
            agent_id,
            allowed_sensitivity: MemorySensitivity::Internal,
            granted_memory_ids: BTreeSet::new(),
            now: Utc::now(),
        };
        let allocation = store
            .allocate(&context, "deterministic evidence-first process", 128)
            .await
            .unwrap();
        assert!(allocation.selected.is_empty());
        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_memory_versions WHERE memory_id = ?")
                .bind(record.id.to_string())
                .fetch_one(&store.pool)
                .await
                .unwrap();
        assert_eq!(remaining, 0);
    }

    #[tokio::test]
    async fn memory_grants_cannot_cross_workspace_boundaries() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let account = storage
            .ensure_firebase_user("memory-test", "memory-store-user-2")
            .await
            .unwrap();
        let store = MemoryStore::new(storage.chat_memory_store().sqlite_pool());
        let record = agent_memory(account.workspace.id, Uuid::new_v4());
        store.put(record.clone(), "human:test").await.unwrap();
        assert!(matches!(
            store
                .grant(Uuid::new_v4(), record.id, Uuid::new_v4(), "human:test")
                .await,
            Err(MemoryStoreError::NotFound)
        ));
    }
}
