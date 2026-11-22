use crate::{
    audit::AuditEvent,
    model_registry::{AssignmentDecision, ModelEvaluation, ModelProvider, ModelRecord, ModelStat},
    chat_memory::ChatMemoryStore,
    model::{Agent, Artifact, Task, Workspace},
    supervisor::SupervisorRun,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    Row, SqlitePool,
};
use std::{str::FromStr, sync::Arc, time::Duration};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("SQLite storage error: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("SQLite migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
    #[error("record serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("stored audit event contains an invalid identifier")]
    InvalidStoredIdentifier,
    #[error("stored audit event contains an invalid timestamp")]
    InvalidStoredTimestamp,
    #[error("stored audit sequence is negative")]
    InvalidStoredSequence,
    #[error("stored entity kind does not match its table")]
    InvalidStoredEntityKind,
    #[error("identity storage is unavailable")]
    IdentityStorageUnavailable,
    #[error("Firebase UID is already bound to a different Firebase project")]
    IdentityProjectMismatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Workspace,
    Agent,
    Artifact,
    Task,
    SupervisorRun,
    Provider,
    Model,
    ModelEvaluation,
    ModelStat,
    AssignmentDecision,
}

impl EntityKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Agent => "agent",
            Self::Artifact => "artifact",
            Self::Task => "task",
            Self::SupervisorRun => "supervisor_run",
            Self::Provider => "provider",
            Self::Model => "model",
            Self::ModelEvaluation => "model_evaluation",
            Self::ModelStat => "model_stat",
            Self::AssignmentDecision => "assignment_decision",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "record", rename_all = "snake_case")]
pub enum StoredEntity {
    Workspace(Workspace),
    Agent(Agent),
    Artifact(Artifact),
    Task(Task),
    SupervisorRun(SupervisorRun),
    Provider(ModelProvider),
    Model(ModelRecord),
    ModelEvaluation(ModelEvaluation),
    ModelStat(ModelStat),
    AssignmentDecision(AssignmentDecision),
}

impl StoredEntity {
    fn kind(&self) -> EntityKind {
        match self {
            Self::Workspace(_) => EntityKind::Workspace,
            Self::Agent(_) => EntityKind::Agent,
            Self::Artifact(_) => EntityKind::Artifact,
            Self::Task(_) => EntityKind::Task,
            Self::SupervisorRun(_) => EntityKind::SupervisorRun,
            Self::Provider(_) => EntityKind::Provider,
            Self::Model(_) => EntityKind::Model,
            Self::ModelEvaluation(_) => EntityKind::ModelEvaluation,
            Self::ModelStat(_) => EntityKind::ModelStat,
            Self::AssignmentDecision(_) => EntityKind::AssignmentDecision,
        }
    }

    fn id(&self) -> Uuid {
        match self {
            Self::Workspace(record) => record.id,
            Self::Agent(record) => record.id,
            Self::Artifact(record) => record.id,
            Self::Task(record) => record.id,
            Self::SupervisorRun(record) => record.id,
            Self::Provider(record) => record.id,
            Self::Model(record) => record.id,
            Self::ModelEvaluation(record) => record.id,
            Self::ModelStat(record) => record.id,
            Self::AssignmentDecision(record) => record.id,
        }
    }

    fn workspace_id(&self) -> Option<Uuid> {
        match self {
            Self::Workspace(_) => None,
            Self::Agent(record) => Some(record.workspace_id),
            Self::Artifact(record) => Some(record.workspace_id),
            Self::Task(record) => Some(record.workspace_id),
            Self::SupervisorRun(record) => Some(record.workspace_id),
            Self::Provider(_) | Self::Model(_) | Self::ModelEvaluation(_) | Self::ModelStat(_) | Self::AssignmentDecision(_) => None,
        }
    }

    fn created_at(&self) -> DateTime<Utc> {
        match self {
            Self::Workspace(record) => record.created_at,
            Self::Agent(record) => record.created_at,
            Self::Artifact(record) => record.captured_at,
            Self::Task(record) => record.created_at,
            Self::SupervisorRun(record) => record.created_at,
            Self::Provider(record) => record.created_at,
            Self::Model(record) => record.created_at,
            Self::ModelEvaluation(record) => record.run_at,
            Self::ModelStat(record) => record.last_updated,
            Self::AssignmentDecision(record) => record.created_at,
        }
    }
}

/// A single atomic write: the entity change and its audit event either both
/// commit or neither does. Audit rows are immutable after insertion.
#[derive(Debug, Clone)]
pub struct StorageMutation {
    pub entity: StoredEntity,
    pub related_entities: Vec<StoredEntity>,
    pub audit: AuditEvent,
}

#[derive(Debug, Clone)]
pub struct UserWorkspace {
    pub user_id: Uuid,
    pub workspace: Workspace,
}

#[async_trait]
pub trait Storage: Send + Sync {
    fn chat_memory_store(&self) -> ChatMemoryStore;
    async fn apply(&self, mutation: StorageMutation) -> Result<(), StorageError>;

    async fn list_entities(
        &self,
        kind: EntityKind,
        workspace_id: Option<Uuid>,
    ) -> Result<Vec<StoredEntity>, StorageError>;

    async fn list_audit_events(&self, workspace_id: Uuid) -> Result<Vec<AuditEvent>, StorageError>;

    async fn ensure_firebase_user(
        &self,
        project_id: &str,
        uid: &str,
    ) -> Result<UserWorkspace, StorageError>;

    async fn is_workspace_member(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<bool, StorageError>;

    async fn list_user_workspaces(&self, user_id: Uuid) -> Result<Vec<Workspace>, StorageError>;

    async fn create_workspace_for_user(
        &self,
        user_id: Uuid,
        name: String,
        actor: String,
    ) -> Result<Workspace, StorageError>;

    async fn grant_platform_admin(&self, user_id: Uuid, firebase_uid: &str) -> Result<(), StorageError>;
    async fn is_platform_admin(&self, user_id: Uuid) -> Result<bool, StorageError>;
}

#[derive(Clone)]
pub struct SqliteStorage {
    pool: SqlitePool,
}

impl SqliteStorage {
    pub async fn connect(database_url: &str) -> Result<Self, StorageError> {
        let options = SqliteConnectOptions::from_str(database_url)?
            .create_if_missing(true)
            .busy_timeout(Duration::from_secs(10));
        let pool = SqlitePoolOptions::new()
            .max_connections(5)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub async fn connect_in_memory() -> Result<Self, StorageError> {
        let options = SqliteConnectOptions::from_str("sqlite::memory:")?
            .busy_timeout(Duration::from_secs(10));
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&pool).await?;
        Ok(Self { pool })
    }

    pub fn shared(self) -> Arc<dyn Storage> {
        Arc::new(self)
    }
}

#[async_trait]
impl Storage for SqliteStorage {
    fn chat_memory_store(&self) -> ChatMemoryStore {
        ChatMemoryStore::new(self.pool.clone())
    }

    async fn apply(&self, mutation: StorageMutation) -> Result<(), StorageError> {
        let mut transaction = self.pool.begin().await?;
        for entity in std::iter::once(mutation.entity).chain(mutation.related_entities) {
            let payload = serde_json::to_string(&entity)?;
            let workspace_id = entity.workspace_id().map(|id| id.to_string());
            let created_at = entity.created_at().to_rfc3339();
            sqlx::query(
                "INSERT INTO entities (kind, id, workspace_id, created_at, payload_json) \
                 VALUES (?, ?, ?, ?, ?) \
                 ON CONFLICT(kind, id) DO UPDATE SET \
                 workspace_id = excluded.workspace_id, \
                 created_at = excluded.created_at, \
                 payload_json = excluded.payload_json",
            )
            .bind(entity.kind().as_str())
            .bind(entity.id().to_string())
            .bind(workspace_id)
            .bind(created_at)
            .bind(payload.clone())
            .execute(&mut *transaction)
            .await?;

            match &entity {
                StoredEntity::Provider(item) => {
                    sqlx::query("INSERT INTO model_providers (id, name, base_url, model_discovery_url, pricing_source_url, api_key_env, discovery_enabled, rate_limit_per_minute, status, data_terms_json, terms_reviewed_at, reviewed_by, payload_json, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET name=excluded.name, base_url=excluded.base_url, model_discovery_url=excluded.model_discovery_url, pricing_source_url=excluded.pricing_source_url, api_key_env=excluded.api_key_env, discovery_enabled=excluded.discovery_enabled, rate_limit_per_minute=excluded.rate_limit_per_minute, status=excluded.status, data_terms_json=excluded.data_terms_json, terms_reviewed_at=excluded.terms_reviewed_at, reviewed_by=excluded.reviewed_by, payload_json=excluded.payload_json, updated_at=excluded.updated_at")
                        .bind(item.id.to_string()).bind(&item.name).bind(&item.base_url)
                        .bind(&item.model_discovery_url).bind(&item.pricing_source_url).bind(&item.api_key_env)
                        .bind(item.discovery_enabled).bind(i64::from(item.rate_limit_per_minute))
                        .bind(&item.status).bind(serde_json::to_string(&item.data_terms)?)
                        .bind(item.terms_reviewed_at.map(|value| value.to_rfc3339()))
                        .bind(item.reviewed_by.map(|value| value.to_string()))
                        .bind(payload).bind(item.updated_at.to_rfc3339())
                        .execute(&mut *transaction).await?;
                }
                StoredEntity::Model(item) => {
                    let status = match &item.status { crate::model_registry::RegistryStatus::Candidate => "candidate", crate::model_registry::RegistryStatus::Approved => "approved", crate::model_registry::RegistryStatus::Disabled => "disabled" };
                    sqlx::query("INSERT INTO models (id, provider_id, model_name, status, categories_json, abilities_json, context_length, input_cost_per_million, output_cost_per_million, version, flagged_for_review, payload_json, updated_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET provider_id=excluded.provider_id, model_name=excluded.model_name, status=excluded.status, categories_json=excluded.categories_json, abilities_json=excluded.abilities_json, context_length=excluded.context_length, input_cost_per_million=excluded.input_cost_per_million, output_cost_per_million=excluded.output_cost_per_million, version=excluded.version, flagged_for_review=excluded.flagged_for_review, payload_json=excluded.payload_json, updated_at=excluded.updated_at")
                        .bind(item.id.to_string()).bind(item.provider_id.to_string()).bind(&item.model_name).bind(status)
                        .bind(serde_json::to_string(&item.categories)?).bind(serde_json::to_string(&item.abilities)?)
                        .bind(item.context_length as i64).bind(item.input_cost_per_million as i64).bind(item.output_cost_per_million as i64)
                        .bind(&item.version).bind(item.flagged_for_review).bind(payload).bind(item.updated_at.to_rfc3339())
                        .execute(&mut *transaction).await?;
                }
                StoredEntity::ModelEvaluation(item) => {
                    sqlx::query("INSERT INTO model_evaluations (id, model_id, category, test_set_version, score, run_at, cost_micros, payload_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(id) DO UPDATE SET score=excluded.score, cost_micros=excluded.cost_micros, payload_json=excluded.payload_json")
                        .bind(item.id.to_string()).bind(item.model_id.to_string()).bind(&item.category).bind(&item.test_set_version)
                        .bind(item.score).bind(item.run_at.to_rfc3339()).bind(item.cost_micros as i64).bind(payload)
                        .execute(&mut *transaction).await?;
                }
                StoredEntity::ModelStat(item) => {
                    let role = match &item.role { crate::model_registry::AssignmentRole::ChecklistWriter => "checklist_writer", crate::model_registry::AssignmentRole::Worker => "worker", crate::model_registry::AssignmentRole::Supervisor => "supervisor" };
                    sqlx::query("INSERT INTO model_stats (id, model_id, category, role, runs, completion_rate, supervisor_pass_rate, user_retry_rate, cost_per_run_micros, latency_ms, last_updated, payload_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(model_id, category, role) DO UPDATE SET runs=excluded.runs, completion_rate=excluded.completion_rate, supervisor_pass_rate=excluded.supervisor_pass_rate, user_retry_rate=excluded.user_retry_rate, cost_per_run_micros=excluded.cost_per_run_micros, latency_ms=excluded.latency_ms, last_updated=excluded.last_updated, payload_json=excluded.payload_json")
                        .bind(item.id.to_string()).bind(item.model_id.to_string()).bind(&item.category).bind(role)
                        .bind(item.runs as i64).bind(item.completion_rate).bind(item.supervisor_pass_rate).bind(item.user_retry_rate)
                        .bind(item.cost_per_run_micros as i64).bind(item.latency_ms as i64).bind(item.last_updated.to_rfc3339()).bind(payload)
                        .execute(&mut *transaction).await?;
                }
                StoredEntity::AssignmentDecision(item) => {
                    let role = match item.role { crate::model_registry::AssignmentRole::ChecklistWriter => "checklist_writer", crate::model_registry::AssignmentRole::Worker => "worker", crate::model_registry::AssignmentRole::Supervisor => "supervisor" };
                    sqlx::query("INSERT INTO assignment_decisions (id, run_id, role, slot, candidates_json, filters_json, chosen_model_id, reason, estimated_cost_micros, created_at, payload_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT(run_id, role, slot) DO UPDATE SET candidates_json=excluded.candidates_json, filters_json=excluded.filters_json, chosen_model_id=excluded.chosen_model_id, reason=excluded.reason, estimated_cost_micros=excluded.estimated_cost_micros, created_at=excluded.created_at, payload_json=excluded.payload_json")
                        .bind(item.id.to_string()).bind(item.run_id.to_string()).bind(role).bind(item.slot as i64)
                        .bind(serde_json::to_string(&item.candidates_considered)?).bind(serde_json::to_string(&item.filters_applied)?)
                        .bind(item.chosen_model_id.to_string()).bind(&item.reason).bind(item.estimated_cost_micros as i64)
                        .bind(item.created_at.to_rfc3339()).bind(payload)
                        .execute(&mut *transaction).await?;
                }
                _ => {}
            }
        }

        let audit = mutation.audit;
        sqlx::query(
            "INSERT INTO audit_events \
             (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(audit.event_id.to_string())
        .bind(audit.workspace_id.map(|id| id.to_string()))
        .bind(audit.actor)
        .bind(audit.event_type)
        .bind(audit.entity_type)
        .bind(audit.entity_id)
        .bind(audit.occurred_at.to_rfc3339())
        .bind(serde_json::to_string(&audit.metadata)?)
        .execute(&mut *transaction)
        .await?;

        transaction.commit().await?;
        Ok(())
    }

    async fn list_entities(
        &self,
        kind: EntityKind,
        workspace_id: Option<Uuid>,
    ) -> Result<Vec<StoredEntity>, StorageError> {
        let workspace_id = workspace_id.map(|id| id.to_string());
        let rows = sqlx::query(
            "SELECT payload_json FROM entities \
             WHERE kind = ? AND (? IS NULL OR workspace_id = ?) \
             ORDER BY created_at ASC, id ASC",
        )
        .bind(kind.as_str())
        .bind(workspace_id.as_deref())
        .bind(workspace_id.as_deref())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let payload: String = row.try_get("payload_json")?;
                Ok(serde_json::from_str(&payload)?)
            })
            .collect()
    }

    async fn list_audit_events(&self, workspace_id: Uuid) -> Result<Vec<AuditEvent>, StorageError> {
        let rows = sqlx::query(
            "SELECT sequence, event_id, workspace_id, actor, event_type, entity_type, \
             entity_id, occurred_at, metadata_json FROM audit_events \
             WHERE workspace_id = ? ORDER BY sequence ASC",
        )
        .bind(workspace_id.to_string())
        .fetch_all(&self.pool)
        .await?;

        rows.into_iter()
            .map(|row| {
                let sequence: i64 = row.try_get("sequence")?;
                let event_id: String = row.try_get("event_id")?;
                let workspace_id: Option<String> = row.try_get("workspace_id")?;
                let occurred_at: String = row.try_get("occurred_at")?;
                let metadata: String = row.try_get("metadata_json")?;
                Ok(AuditEvent {
                    sequence: u64::try_from(sequence)
                        .map_err(|_| StorageError::InvalidStoredSequence)?,
                    event_id: Uuid::parse_str(&event_id)
                        .map_err(|_| StorageError::InvalidStoredIdentifier)?,
                    workspace_id: workspace_id
                        .map(|value| Uuid::parse_str(&value))
                        .transpose()
                        .map_err(|_| StorageError::InvalidStoredIdentifier)?,
                    actor: row.try_get("actor")?,
                    event_type: row.try_get("event_type")?,
                    entity_type: row.try_get("entity_type")?,
                    entity_id: row.try_get("entity_id")?,
                    occurred_at: DateTime::parse_from_rfc3339(&occurred_at)
                        .map_err(|_| StorageError::InvalidStoredTimestamp)?
                        .with_timezone(&Utc),
                    metadata: serde_json::from_str(&metadata)?,
                })
            })
            .collect()
    }

    async fn ensure_firebase_user(
        &self,
        project_id: &str,
        uid: &str,
    ) -> Result<UserWorkspace, StorageError> {
        let mut tx = self.pool.begin().await?;
        let now = Utc::now();
        let candidate_user_id = Uuid::new_v4();
        sqlx::query(
            "INSERT INTO users (id, auth_provider, provider_subject, created_at, firebase_project_id) VALUES (?, 'firebase', ?, ?, ?) ON CONFLICT(auth_provider, provider_subject) DO NOTHING"
        )
        .bind(candidate_user_id.to_string())
         .bind(uid)
        .bind(now.to_rfc3339())
        .bind(project_id)
        .execute(&mut *tx).await?;

        let row = sqlx::query("SELECT id, personal_workspace_id, firebase_project_id FROM users WHERE auth_provider = 'firebase' AND provider_subject = ?")
             .bind(uid)
            .fetch_one(&mut *tx).await?;
        let user_id_text: String = row.try_get("id")?;
        let user_id =
            Uuid::parse_str(&user_id_text).map_err(|_| StorageError::InvalidStoredIdentifier)?;
        let linked_workspace: Option<String> = row.try_get("personal_workspace_id")?;
        let stored_project: Option<String> = row.try_get("firebase_project_id")?;
        if stored_project.as_deref() != Some(project_id) {
            return Err(StorageError::IdentityProjectMismatch);
        }

        let workspace = if let Some(workspace_id) = linked_workspace {
            let workspace_id = Uuid::parse_str(&workspace_id)
                .map_err(|_| StorageError::InvalidStoredIdentifier)?;
            let row = sqlx::query(
                "SELECT payload_json FROM entities WHERE kind = 'workspace' AND id = ?",
            )
            .bind(workspace_id.to_string())
            .fetch_one(&mut *tx)
            .await?;
            let payload: String = row.try_get("payload_json")?;
            match serde_json::from_str::<StoredEntity>(&payload)? {
                StoredEntity::Workspace(workspace) => workspace,
                _ => return Err(StorageError::InvalidStoredEntityKind),
            }
        } else {
            let workspace = Workspace {
                id: Uuid::new_v4(),
                name: "Personal workspace".into(),
                created_at: now,
            };
            let entity = StoredEntity::Workspace(workspace.clone());
            let payload = serde_json::to_string(&entity)?;
            sqlx::query("INSERT INTO entities (kind, id, workspace_id, created_at, payload_json) VALUES ('workspace', ?, NULL, ?, ?)")
                .bind(workspace.id.to_string())
                .bind(workspace.created_at.to_rfc3339())
                .bind(payload)
                .execute(&mut *tx).await?;
            sqlx::query("UPDATE users SET personal_workspace_id = ? WHERE id = ? AND personal_workspace_id IS NULL")
                .bind(workspace.id.to_string())
                .bind(user_id.to_string())
                .execute(&mut *tx).await?;
            sqlx::query("INSERT INTO workspace_memberships (workspace_id, user_id, role, created_at) VALUES (?, ?, 'owner', ?) ON CONFLICT(workspace_id, user_id) DO NOTHING")
                .bind(workspace.id.to_string())
                .bind(user_id.to_string())
                .bind(now.to_rfc3339())
                .execute(&mut *tx).await?;
            let event = AuditEvent {
                sequence: 0,
                event_id: Uuid::new_v4(),
                workspace_id: Some(workspace.id),
                actor: format!("human:{user_id}"),
                event_type: "workspace.personal_created".into(),
                entity_type: "workspace".into(),
                entity_id: workspace.id.to_string(),
                occurred_at: now,
                metadata: serde_json::json!({"provisioned_by": "firebase_auth"}),
            };
            sqlx::query("INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
                .bind(event.event_id.to_string())
                .bind(event.workspace_id.map(|id| id.to_string()))
                .bind(event.actor)
                .bind(event.event_type)
                .bind(event.entity_type)
                .bind(event.entity_id)
                .bind(event.occurred_at.to_rfc3339())
                .bind(serde_json::to_string(&event.metadata)?)
                .execute(&mut *tx).await?;
            workspace
        };
        tx.commit().await?;
        Ok(UserWorkspace { user_id, workspace })
    }

    async fn is_workspace_member(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
    ) -> Result<bool, StorageError> {
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE user_id = ? AND workspace_id = ?",
        )
        .bind(user_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    async fn list_user_workspaces(&self, user_id: Uuid) -> Result<Vec<Workspace>, StorageError> {
        let rows = sqlx::query(
            "SELECT e.payload_json FROM workspace_memberships m JOIN entities e ON e.kind = 'workspace' AND e.id = m.workspace_id WHERE m.user_id = ? ORDER BY m.created_at ASC"
        )
        .bind(user_id.to_string())
        .fetch_all(&self.pool).await?;
        rows.into_iter()
            .map(|row| {
                let payload: String = row.try_get("payload_json")?;
                match serde_json::from_str::<StoredEntity>(&payload)? {
                    StoredEntity::Workspace(workspace) => Ok(workspace),
                    _ => Err(StorageError::InvalidStoredEntityKind),
                }
            })
            .collect()
    }

    async fn create_workspace_for_user(
        &self,
        user_id: Uuid,
        name: String,
        actor: String,
    ) -> Result<Workspace, StorageError> {
        let mut tx = self.pool.begin().await?;
        let workspace = Workspace {
            id: Uuid::new_v4(),
            name,
            created_at: Utc::now(),
        };
        let entity = StoredEntity::Workspace(workspace.clone());
        sqlx::query("INSERT INTO entities (kind, id, workspace_id, created_at, payload_json) VALUES ('workspace', ?, NULL, ?, ?)")
            .bind(workspace.id.to_string())
            .bind(workspace.created_at.to_rfc3339())
            .bind(serde_json::to_string(&entity)?)
            .execute(&mut *tx).await?;
        sqlx::query("INSERT INTO workspace_memberships (workspace_id, user_id, role, created_at) VALUES (?, ?, 'owner', ?)")
            .bind(workspace.id.to_string())
            .bind(user_id.to_string())
            .bind(workspace.created_at.to_rfc3339())
            .execute(&mut *tx).await?;
        let event = AuditEvent {
            sequence: 0,
            event_id: Uuid::new_v4(),
            workspace_id: Some(workspace.id),
            actor,
            event_type: "workspace.created".into(),
            entity_type: "workspace".into(),
            entity_id: workspace.id.to_string(),
            occurred_at: workspace.created_at,
            metadata: serde_json::json!({"name": workspace.name}),
        };
        sqlx::query("INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")
            .bind(event.event_id.to_string())
            .bind(event.workspace_id.map(|id| id.to_string()))
            .bind(event.actor)
            .bind(event.event_type)
            .bind(event.entity_type)
            .bind(event.entity_id)
            .bind(event.occurred_at.to_rfc3339())
            .bind(serde_json::to_string(&event.metadata)?)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(workspace)
    }

    async fn grant_platform_admin(&self, user_id: Uuid, firebase_uid: &str) -> Result<(), StorageError> {
        let mut tx = self.pool.begin().await?;
        let now = Utc::now();
        sqlx::query("INSERT INTO platform_admins (user_id, firebase_uid, granted_at, granted_by) VALUES (?, ?, ?, 'bootstrap:AXON_ADMIN_UIDS') ON CONFLICT(user_id) DO NOTHING")
            .bind(user_id.to_string())
            .bind(firebase_uid)
            .bind(now.to_rfc3339())
            .execute(&mut *tx).await?;
        let event = AuditEvent::attributed(
            "system:admin_allowlist_bootstrap", None, "platform_admin.granted",
            "platform_admin", user_id.to_string(),
            serde_json::json!({"firebase_uid": firebase_uid, "source": "AXON_ADMIN_UIDS"}),
        );
        sqlx::query("INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, NULL, ?, ?, ?, ?, ?, ?)")
            .bind(event.event_id.to_string()).bind(event.actor).bind(event.event_type)
            .bind(event.entity_type).bind(event.entity_id).bind(event.occurred_at.to_rfc3339())
            .bind(serde_json::to_string(&event.metadata)?)
            .execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    async fn is_platform_admin(&self, user_id: Uuid) -> Result<bool, StorageError> {
        let exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM platform_admins WHERE user_id = ?")
            .bind(user_id.to_string()).fetch_optional(&self.pool).await?;
        Ok(exists.is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::Workspace;
    use serde_json::json;

    fn mutation(workspace_id: Uuid, event_id: Uuid) -> StorageMutation {
        let now = Utc::now();
        StorageMutation {
            entity: StoredEntity::Workspace(Workspace {
                id: workspace_id,
                name: "Axon".into(),
                created_at: now,
            }),
            related_entities: Vec::new(),
            audit: AuditEvent {
                sequence: 0,
                event_id,
                workspace_id: Some(workspace_id),
                actor: "human:test-user".into(),
                event_type: "workspace.created".into(),
                entity_type: "workspace".into(),
                entity_id: workspace_id.to_string(),
                occurred_at: now,
                metadata: json!({"name": "Axon"}),
            },
        }
    }

    #[tokio::test]
    async fn sqlite_persists_entities_and_audit_events() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let workspace_id = Uuid::new_v4();
        storage
            .apply(mutation(workspace_id, Uuid::new_v4()))
            .await
            .unwrap();

        let entities = storage
            .list_entities(EntityKind::Workspace, None)
            .await
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert!(matches!(&entities[0], StoredEntity::Workspace(item) if item.id == workspace_id));

        let events = storage.list_audit_events(workspace_id).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].sequence, 1);
        assert_eq!(events[0].actor, "human:test-user");
    }

    #[tokio::test]
    async fn entity_mutation_rolls_back_when_audit_insert_fails() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let first_workspace = Uuid::new_v4();
        let duplicate_event = Uuid::new_v4();
        storage
            .apply(mutation(first_workspace, duplicate_event))
            .await
            .unwrap();

        let second_workspace = Uuid::new_v4();
        assert!(storage
            .apply(mutation(second_workspace, duplicate_event))
            .await
            .is_err());

        let entities = storage
            .list_entities(EntityKind::Workspace, None)
            .await
            .unwrap();
        assert_eq!(entities.len(), 1);
        assert!(
            matches!(&entities[0], StoredEntity::Workspace(item) if item.id == first_workspace)
        );
        assert!(storage
            .list_audit_events(second_workspace)
            .await
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn audit_events_reject_update_and_delete() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let workspace_id = Uuid::new_v4();
        storage
            .apply(mutation(workspace_id, Uuid::new_v4()))
            .await
            .unwrap();

        let update =
            sqlx::query("UPDATE audit_events SET actor = 'changed' WHERE workspace_id = ?")
                .bind(workspace_id.to_string())
                .execute(&storage.pool)
                .await;
        assert!(update.is_err());

        let delete = sqlx::query("DELETE FROM audit_events WHERE workspace_id = ?")
            .bind(workspace_id.to_string())
            .execute(&storage.pool)
            .await;
        assert!(delete.is_err());
    }

    #[tokio::test]
    async fn platform_admin_grant_is_persisted_and_audited() {
        let storage = SqliteStorage::connect_in_memory().await.unwrap();
        let user_id = Uuid::new_v4();
        storage.grant_platform_admin(user_id, "configured-admin-uid").await.unwrap();
        assert!(storage.is_platform_admin(user_id).await.unwrap());
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM audit_events WHERE event_type = 'platform_admin.granted'")
            .fetch_one(&storage.pool).await.unwrap();
        assert_eq!(count, 1);
    }

    #[tokio::test]
    async fn firebase_first_login_is_idempotent_under_concurrent_requests() {
        let database_path =
            std::env::temp_dir().join(format!("axon-identity-{}.db", Uuid::new_v4()));
        let database_url = format!("sqlite://{}", database_path.display());
        let storage = SqliteStorage::connect(&database_url).await.unwrap();
        let (first, second) = tokio::join!(
            storage.ensure_firebase_user("axon-project", "same-firebase-uid"),
            storage.ensure_firebase_user("axon-project", "same-firebase-uid"),
        );
        let first = first.unwrap();
        let second = second.unwrap();
        assert_eq!(first.user_id, second.user_id);
        assert_eq!(first.workspace.id, second.workspace.id);
        assert!(storage
            .is_workspace_member(first.user_id, first.workspace.id)
            .await
            .unwrap());
        assert_eq!(
            storage
                .list_user_workspaces(first.user_id)
                .await
                .unwrap()
                .len(),
            1
        );
        let audit = storage.list_audit_events(first.workspace.id).await.unwrap();
        assert_eq!(audit.len(), 1);
        assert_eq!(audit[0].actor, format!("human:{}", first.user_id));
        drop(storage);
        std::fs::remove_file(database_path).unwrap();
    }
}
