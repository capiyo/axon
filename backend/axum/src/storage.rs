use crate::{
    audit::AuditEvent,
    chat_memory::ChatMemoryStore,
    memory_architecture::MemoryEpochCoordinator,
    memory_outbox::MemoryOutbox,
    model::{
        Agent, Artifact, Project, ProjectMembership, ProjectRole, Task, Workspace,
        WorkspaceMembership,
    },
};
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
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
    #[error("project was not found in the requested workspace")]
    ProjectNotFound,
    #[error("project membership or role does not authorize this operation")]
    ProjectAuthorizationDenied,
    #[error("target user is not an active member of the project workspace")]
    ProjectWorkspaceMembershipRequired,
}

#[derive(Debug, Clone, Serialize)]
pub struct IssuedAgentCredential {
    pub credential_id: Uuid,
    pub token: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct AgentCredentialIdentity {
    pub credential_id: Uuid,
    pub agent_id: Uuid,
    pub workspace_id: Uuid,
    pub owner_user_id: Uuid,
    pub firebase_uid: String,
    pub firebase_project_id: String,
    pub personal_workspace_id: Uuid,
}

fn agent_credential_hash(secret: &str) -> String {
    let digest = Sha256::digest(secret.as_bytes());
    format!("{digest:x}")
}

fn parse_agent_credential_token(token: &str) -> Option<(Uuid, String)> {
    let token = token.strip_prefix("axn_")?;
    let (credential_id, secret) = token.split_once('.')?;
    if secret.len() != 32 || !secret.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    Some((Uuid::parse_str(credential_id).ok()?, secret.to_owned()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntityKind {
    Workspace,
    Agent,
    Artifact,
    Task,
}

impl EntityKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Agent => "agent",
            Self::Artifact => "artifact",
            Self::Task => "task",
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
}

impl StoredEntity {
    fn kind(&self) -> EntityKind {
        match self {
            Self::Workspace(_) => EntityKind::Workspace,
            Self::Agent(_) => EntityKind::Agent,
            Self::Artifact(_) => EntityKind::Artifact,
            Self::Task(_) => EntityKind::Task,
        }
    }

    fn id(&self) -> Uuid {
        match self {
            Self::Workspace(record) => record.id,
            Self::Agent(record) => record.id,
            Self::Artifact(record) => record.id,
            Self::Task(record) => record.id,
        }
    }

    fn workspace_id(&self) -> Option<Uuid> {
        match self {
            Self::Workspace(_) => None,
            Self::Agent(record) => Some(record.workspace_id),
            Self::Artifact(record) => Some(record.workspace_id),
            Self::Task(record) => Some(record.workspace_id),
        }
    }

    fn created_at(&self) -> DateTime<Utc> {
        match self {
            Self::Workspace(record) => record.created_at,
            Self::Agent(record) => record.created_at,
            Self::Artifact(record) => record.captured_at,
            Self::Task(record) => record.created_at,
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
    async fn ping(&self) -> Result<(), StorageError> {
        Err(StorageError::IdentityStorageUnavailable)
    }
    fn chat_memory_store(&self) -> ChatMemoryStore;
    fn memory_epoch_coordinator(&self) -> MemoryEpochCoordinator;
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

    async fn add_workspace_member(
        &self,
        workspace_id: Uuid,
        actor_user_id: Uuid,
        target_user_id: Uuid,
    ) -> Result<WorkspaceMembership, StorageError>;

    async fn issue_agent_credential(
        &self,
        workspace_id: Uuid,
        agent_id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<IssuedAgentCredential, StorageError>;

    async fn rotate_agent_credential(
        &self,
        workspace_id: Uuid,
        agent_id: Uuid,
        credential_id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<Option<IssuedAgentCredential>, StorageError>;

    async fn revoke_agent_credential(
        &self,
        workspace_id: Uuid,
        agent_id: Uuid,
        credential_id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<bool, StorageError>;

    async fn authenticate_agent_credential(
        &self,
        token: &str,
    ) -> Result<Option<AgentCredentialIdentity>, StorageError>;

    async fn revoke_workspace_member(
        &self,
        workspace_id: Uuid,
        actor_user_id: Uuid,
        target_user_id: Uuid,
    ) -> Result<bool, StorageError>;

    async fn create_project_for_user(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
        name: String,
    ) -> Result<Project, StorageError>;

    async fn list_user_projects(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> Result<Vec<Project>, StorageError>;

    async fn is_project_member(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, StorageError>;

    async fn set_project_member(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
        actor_user_id: Uuid,
        target_user_id: Uuid,
        role: ProjectRole,
    ) -> Result<ProjectMembership, StorageError>;

    async fn revoke_project_member(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
        actor_user_id: Uuid,
        target_user_id: Uuid,
    ) -> Result<bool, StorageError>;
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

    pub fn sqlite_pool(&self) -> SqlitePool {
        self.pool.clone()
    }

    pub fn memory_epoch_coordinator(&self) -> MemoryEpochCoordinator {
        MemoryEpochCoordinator::new(self.pool.clone())
    }

    pub fn memory_outbox(&self) -> MemoryOutbox {
        MemoryOutbox::new(self.pool.clone())
    }

    pub fn shared(self) -> Arc<dyn Storage> {
        Arc::new(self)
    }

    #[cfg(test)]
    pub(crate) fn test_pool(&self) -> &SqlitePool {
        &self.pool
    }
}

#[async_trait]
impl Storage for SqliteStorage {
    async fn ping(&self) -> Result<(), StorageError> {
        sqlx::query("SELECT 1").execute(&self.pool).await?;
        Ok(())
    }

    fn chat_memory_store(&self) -> ChatMemoryStore {
        ChatMemoryStore::new(self.pool.clone())
    }

    fn memory_epoch_coordinator(&self) -> MemoryEpochCoordinator {
        MemoryEpochCoordinator::new(self.pool.clone())
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
            .bind(payload)
            .execute(&mut *transaction)
            .await?;
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

    async fn issue_agent_credential(
        &self,
        workspace_id: Uuid,
        agent_id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<IssuedAgentCredential, StorageError> {
        let mut tx = self.pool.begin().await?;
        let owner_is_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(owner_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if owner_is_member.is_none() {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let payload: Option<String> = sqlx::query_scalar(
            "SELECT payload_json FROM entities WHERE kind = 'agent' AND id = ? AND workspace_id = ?",
        )
        .bind(agent_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(payload) = payload else {
            return Err(StorageError::ProjectNotFound);
        };
        let entity: StoredEntity = serde_json::from_str(&payload)?;
        if !matches!(entity, StoredEntity::Agent(ref agent)
            if agent.id == agent_id && agent.workspace_id == workspace_id
                && agent.created_by_user_id == Some(owner_user_id))
        {
            return Err(StorageError::ProjectAuthorizationDenied);
        }

        let credential_id = Uuid::new_v4();
        let secret = Uuid::new_v4().simple().to_string();
        let token = format!("axn_{credential_id}.{secret}");
        let now = Utc::now();
        let expires_at = now + ChronoDuration::days(90);
        sqlx::query(
            "INSERT INTO agent_credentials (id, workspace_id, agent_id, owner_user_id, secret_hash, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(credential_id.to_string())
        .bind(workspace_id.to_string())
        .bind(agent_id.to_string())
        .bind(owner_user_id.to_string())
        .bind(agent_credential_hash(&secret))
        .bind(now.to_rfc3339())
        .bind(expires_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'agent.credential_issued', 'agent', ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(workspace_id.to_string())
        .bind(format!("human:{owner_user_id}"))
        .bind(agent_id.to_string())
        .bind(now.to_rfc3339())
        .bind(serde_json::json!({"credential_id": credential_id, "expires_at": expires_at}).to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(IssuedAgentCredential {
            credential_id,
            token,
            created_at: now,
            expires_at,
        })
    }

    async fn rotate_agent_credential(
        &self,
        workspace_id: Uuid,
        agent_id: Uuid,
        credential_id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<Option<IssuedAgentCredential>, StorageError> {
        let mut tx = self.pool.begin().await?;
        let owner_is_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(owner_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if owner_is_member.is_none() {
            tx.rollback().await?;
            return Ok(None);
        }
        let owner_is_agent_owner: Option<String> = sqlx::query_scalar(
            "SELECT payload_json FROM entities WHERE kind = 'agent' AND id = ? AND workspace_id = ?",
        )
        .bind(agent_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(payload) = owner_is_agent_owner else {
            return Ok(None);
        };
        let entity: StoredEntity = serde_json::from_str(&payload)?;
        if !matches!(entity, StoredEntity::Agent(ref agent)
            if agent.created_by_user_id == Some(owner_user_id))
        {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let now = Utc::now();
        let revoked = sqlx::query(
            "UPDATE agent_credentials SET revoked_at = ? WHERE id = ? AND workspace_id = ? AND agent_id = ? AND owner_user_id = ? AND revoked_at IS NULL",
        )
        .bind(now.to_rfc3339())
        .bind(credential_id.to_string())
        .bind(workspace_id.to_string())
        .bind(agent_id.to_string())
        .bind(owner_user_id.to_string())
        .execute(&mut *tx)
        .await?;
        if revoked.rows_affected() != 1 {
            tx.rollback().await?;
            return Ok(None);
        }
        let new_id = Uuid::new_v4();
        let secret = Uuid::new_v4().simple().to_string();
        let token = format!("axn_{new_id}.{secret}");
        let expires_at = now + ChronoDuration::days(90);
        sqlx::query(
            "INSERT INTO agent_credentials (id, workspace_id, agent_id, owner_user_id, secret_hash, created_at, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id.to_string())
        .bind(workspace_id.to_string())
        .bind(agent_id.to_string())
        .bind(owner_user_id.to_string())
        .bind(agent_credential_hash(&secret))
        .bind(now.to_rfc3339())
        .bind(expires_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'agent.credential_rotated', 'agent', ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(workspace_id.to_string())
        .bind(format!("human:{owner_user_id}"))
        .bind(agent_id.to_string())
        .bind(now.to_rfc3339())
        .bind(serde_json::json!({"revoked_credential_id": credential_id, "credential_id": new_id}).to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(Some(IssuedAgentCredential {
            credential_id: new_id,
            token,
            created_at: now,
            expires_at,
        }))
    }

    async fn revoke_agent_credential(
        &self,
        workspace_id: Uuid,
        agent_id: Uuid,
        credential_id: Uuid,
        owner_user_id: Uuid,
    ) -> Result<bool, StorageError> {
        let now = Utc::now().to_rfc3339();
        let mut tx = self.pool.begin().await?;
        let owner_is_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(owner_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let agent_payload: Option<String> = sqlx::query_scalar(
            "SELECT payload_json FROM entities WHERE kind = 'agent' AND id = ? AND workspace_id = ?",
        )
        .bind(agent_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let is_owner = agent_payload
            .map(|payload| serde_json::from_str::<StoredEntity>(&payload))
            .transpose()?
            .is_some_and(|entity| {
                matches!(entity, StoredEntity::Agent(ref agent)
                if agent.created_by_user_id == Some(owner_user_id))
            });
        if owner_is_member.is_none() || !is_owner {
            tx.rollback().await?;
            return Ok(false);
        }
        let revoked = sqlx::query(
            "UPDATE agent_credentials SET revoked_at = ? WHERE id = ? AND workspace_id = ? AND agent_id = ? AND owner_user_id = ? AND revoked_at IS NULL",
        )
        .bind(&now)
        .bind(credential_id.to_string())
        .bind(workspace_id.to_string())
        .bind(agent_id.to_string())
        .bind(owner_user_id.to_string())
        .execute(&mut *tx)
        .await?;
        if revoked.rows_affected() == 1 {
            sqlx::query(
                "INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'agent.credential_revoked', 'agent', ?, ?, ?)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(workspace_id.to_string())
            .bind(format!("human:{owner_user_id}"))
            .bind(agent_id.to_string())
            .bind(&now)
            .bind(serde_json::json!({"credential_id": credential_id}).to_string())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(revoked.rows_affected() == 1)
    }

    async fn authenticate_agent_credential(
        &self,
        token: &str,
    ) -> Result<Option<AgentCredentialIdentity>, StorageError> {
        let Some((credential_id, secret)) = parse_agent_credential_token(token) else {
            return Ok(None);
        };
        let now = Utc::now();
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT c.agent_id, c.workspace_id, c.owner_user_id,
                    u.provider_subject, u.firebase_project_id, u.personal_workspace_id
             FROM agent_credentials c
             JOIN users u ON u.id = c.owner_user_id
             WHERE c.id = ? AND c.secret_hash = ? AND c.revoked_at IS NULL AND c.expires_at > ?",
        )
        .bind(credential_id.to_string())
        .bind(agent_credential_hash(&secret))
        .bind(now.to_rfc3339())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(None);
        };
        let agent_id_text: String = row.try_get("agent_id")?;
        let workspace_id_text: String = row.try_get("workspace_id")?;
        let owner_user_id_text: String = row.try_get("owner_user_id")?;
        let firebase_uid: String = row.try_get("provider_subject")?;
        let firebase_project_id: Option<String> = row.try_get("firebase_project_id")?;
        let personal_workspace_id: Option<String> = row.try_get("personal_workspace_id")?;
        let agent_id =
            Uuid::parse_str(&agent_id_text).map_err(|_| StorageError::InvalidStoredIdentifier)?;
        let workspace_id = Uuid::parse_str(&workspace_id_text)
            .map_err(|_| StorageError::InvalidStoredIdentifier)?;
        let owner_user_id = Uuid::parse_str(&owner_user_id_text)
            .map_err(|_| StorageError::InvalidStoredIdentifier)?;
        let firebase_project_id =
            firebase_project_id.ok_or(StorageError::InvalidStoredIdentifier)?;
        let personal_workspace_id = Uuid::parse_str(
            personal_workspace_id
                .as_deref()
                .ok_or(StorageError::InvalidStoredIdentifier)?,
        )
        .map_err(|_| StorageError::InvalidStoredIdentifier)?;
        let member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(owner_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if member.is_none() {
            tx.rollback().await?;
            return Ok(None);
        }
        let payload: Option<String> = sqlx::query_scalar(
            "SELECT payload_json FROM entities WHERE kind = 'agent' AND id = ? AND workspace_id = ?",
        )
        .bind(agent_id.to_string())
        .bind(workspace_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(payload) = payload else {
            tx.rollback().await?;
            return Ok(None);
        };
        let entity: StoredEntity = serde_json::from_str(&payload)?;
        if !matches!(entity, StoredEntity::Agent(ref agent)
            if agent.created_by_user_id == Some(owner_user_id))
        {
            tx.rollback().await?;
            return Ok(None);
        }
        sqlx::query("UPDATE agent_credentials SET last_used_at = ? WHERE id = ?")
            .bind(now.to_rfc3339())
            .bind(credential_id.to_string())
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(Some(AgentCredentialIdentity {
            credential_id,
            agent_id,
            workspace_id,
            owner_user_id,
            firebase_uid,
            firebase_project_id,
            personal_workspace_id,
        }))
    }

    async fn add_workspace_member(
        &self,
        workspace_id: Uuid,
        actor_user_id: Uuid,
        target_user_id: Uuid,
    ) -> Result<WorkspaceMembership, StorageError> {
        let mut tx = self.pool.begin().await?;
        let actor_is_owner: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ? AND role = 'owner'",
        )
        .bind(workspace_id.to_string())
        .bind(actor_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if actor_is_owner.is_none() {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let target_exists: Option<i64> = sqlx::query_scalar("SELECT 1 FROM users WHERE id = ?")
            .bind(target_user_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
        if target_exists.is_none() {
            return Err(StorageError::ProjectWorkspaceMembershipRequired);
        }
        let now = Utc::now();
        let inserted = sqlx::query(
            "INSERT INTO workspace_memberships (workspace_id, user_id, role, created_at) VALUES (?, ?, 'member', ?) ON CONFLICT(workspace_id, user_id) DO NOTHING",
        )
        .bind(workspace_id.to_string())
        .bind(target_user_id.to_string())
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        let row = sqlx::query(
            "SELECT role, created_at FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(target_user_id.to_string())
        .fetch_one(&mut *tx)
        .await?;
        let role: String = row.try_get("role")?;
        let created_at: String = row.try_get("created_at")?;
        if inserted.rows_affected() == 1 {
            sqlx::query(
                "INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'workspace.member_added', 'workspace', ?, ?, ?)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(workspace_id.to_string())
            .bind(format!("human:{actor_user_id}"))
            .bind(workspace_id.to_string())
            .bind(now.to_rfc3339())
            .bind(serde_json::json!({"user_id": target_user_id, "role": role}).to_string())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(WorkspaceMembership {
            workspace_id,
            user_id: target_user_id,
            role,
            created_at: DateTime::parse_from_rfc3339(&created_at)
                .map_err(|_| StorageError::InvalidStoredTimestamp)?
                .with_timezone(&Utc),
        })
    }

    async fn revoke_workspace_member(
        &self,
        workspace_id: Uuid,
        actor_user_id: Uuid,
        target_user_id: Uuid,
    ) -> Result<bool, StorageError> {
        let mut tx = self.pool.begin().await?;
        let actor_is_owner: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ? AND role = 'owner'",
        )
        .bind(workspace_id.to_string())
        .bind(actor_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if actor_is_owner.is_none() {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let changed = sqlx::query(
            "DELETE FROM workspace_memberships WHERE workspace_id = ? AND user_id = ? AND role = 'member'",
        )
        .bind(workspace_id.to_string())
        .bind(target_user_id.to_string())
        .execute(&mut *tx)
        .await?;
        if changed.rows_affected() == 1 {
            let now = Utc::now().to_rfc3339();
            sqlx::query(
                "UPDATE project_memberships SET revoked_at = ? WHERE user_id = ? AND project_id IN (SELECT id FROM projects WHERE workspace_id = ?) AND revoked_at IS NULL AND role != 'owner'",
            )
            .bind(&now)
            .bind(target_user_id.to_string())
            .bind(workspace_id.to_string())
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "UPDATE agent_credentials SET revoked_at = ? WHERE owner_user_id = ? AND workspace_id = ? AND revoked_at IS NULL",
            )
            .bind(&now)
            .bind(target_user_id.to_string())
            .bind(workspace_id.to_string())
            .execute(&mut *tx)
            .await?;
            sqlx::query(
                "INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'workspace.member_revoked', 'workspace', ?, ?, ?)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(workspace_id.to_string())
            .bind(format!("human:{actor_user_id}"))
            .bind(workspace_id.to_string())
            .bind(now)
            .bind(serde_json::json!({"user_id": target_user_id}).to_string())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(changed.rows_affected() == 1)
    }

    async fn create_project_for_user(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
        name: String,
    ) -> Result<Project, StorageError> {
        let mut tx = self.pool.begin().await?;
        let workspace_owner: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ? AND role = 'owner'",
        )
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if workspace_owner.is_none() {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let project = Project {
            id: Uuid::new_v4(),
            workspace_id,
            name,
            created_by_user_id: user_id,
            created_at: Utc::now(),
        };
        sqlx::query(
            "INSERT INTO projects (id, workspace_id, name, created_by_user_id, created_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(project.id.to_string())
        .bind(workspace_id.to_string())
        .bind(&project.name)
        .bind(user_id.to_string())
        .bind(project.created_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO project_memberships (project_id, user_id, role, created_at) VALUES (?, ?, 'owner', ?)",
        )
        .bind(project.id.to_string())
        .bind(user_id.to_string())
        .bind(project.created_at.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'project.created', 'project', ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(workspace_id.to_string())
        .bind(format!("human:{user_id}"))
        .bind(project.id.to_string())
        .bind(project.created_at.to_rfc3339())
        .bind(serde_json::json!({"name": project.name}).to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(project)
    }

    async fn list_user_projects(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> Result<Vec<Project>, StorageError> {
        let rows = sqlx::query(
            "SELECT p.id, p.workspace_id, p.name, p.created_by_user_id, p.created_at
             FROM projects p
             JOIN project_memberships m ON m.project_id = p.id
             JOIN workspace_memberships w ON w.workspace_id = p.workspace_id AND w.user_id = m.user_id
             WHERE p.workspace_id = ? AND m.user_id = ? AND m.revoked_at IS NULL
             ORDER BY p.created_at ASC, p.id ASC",
        )
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let id: String = row.try_get("id")?;
                let workspace: String = row.try_get("workspace_id")?;
                let creator: String = row.try_get("created_by_user_id")?;
                let created_at: String = row.try_get("created_at")?;
                Ok(Project {
                    id: Uuid::parse_str(&id).map_err(|_| StorageError::InvalidStoredIdentifier)?,
                    workspace_id: Uuid::parse_str(&workspace)
                        .map_err(|_| StorageError::InvalidStoredIdentifier)?,
                    name: row.try_get("name")?,
                    created_by_user_id: Uuid::parse_str(&creator)
                        .map_err(|_| StorageError::InvalidStoredIdentifier)?,
                    created_at: DateTime::parse_from_rfc3339(&created_at)
                        .map_err(|_| StorageError::InvalidStoredTimestamp)?
                        .with_timezone(&Utc),
                })
            })
            .collect()
    }

    async fn is_project_member(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, StorageError> {
        let exists: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM projects p
             JOIN project_memberships m ON m.project_id = p.id
             JOIN workspace_memberships w ON w.workspace_id = p.workspace_id AND w.user_id = m.user_id
             WHERE p.id = ? AND p.workspace_id = ? AND m.user_id = ? AND m.revoked_at IS NULL",
        )
        .bind(project_id.to_string())
        .bind(workspace_id.to_string())
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await?;
        Ok(exists.is_some())
    }

    async fn set_project_member(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
        actor_user_id: Uuid,
        target_user_id: Uuid,
        role: ProjectRole,
    ) -> Result<ProjectMembership, StorageError> {
        let mut tx = self.pool.begin().await?;
        let project = sqlx::query("SELECT id FROM projects WHERE id = ? AND workspace_id = ?")
            .bind(project_id.to_string())
            .bind(workspace_id.to_string())
            .fetch_optional(&mut *tx)
            .await?;
        if project.is_none() {
            return Err(StorageError::ProjectNotFound);
        }
        let actor_workspace_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(actor_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if actor_workspace_member.is_none() {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let actor_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM project_memberships WHERE project_id = ? AND user_id = ? AND revoked_at IS NULL",
        )
        .bind(project_id.to_string())
        .bind(actor_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        let Some(actor_role) = actor_role else {
            return Err(StorageError::ProjectAuthorizationDenied);
        };
        if actor_role != "owner" && actor_role != "admin" {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        if role != ProjectRole::Member && role != ProjectRole::Viewer && actor_role != "owner" {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let target_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM project_memberships WHERE project_id = ? AND user_id = ? AND revoked_at IS NULL",
        )
        .bind(project_id.to_string())
        .bind(target_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if target_role.as_deref() == Some("owner") && role != ProjectRole::Owner {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        if actor_role == "admin" && matches!(target_role.as_deref(), Some("owner" | "admin")) {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let target_is_workspace_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(target_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if target_is_workspace_member.is_none() {
            return Err(StorageError::ProjectWorkspaceMembershipRequired);
        }
        let now = Utc::now();
        sqlx::query(
            "INSERT INTO project_memberships (project_id, user_id, role, created_at, revoked_at)
             VALUES (?, ?, ?, ?, NULL)
             ON CONFLICT(project_id, user_id) DO UPDATE SET role = excluded.role, revoked_at = NULL",
        )
        .bind(project_id.to_string())
        .bind(target_user_id.to_string())
        .bind(role.as_str())
        .bind(now.to_rfc3339())
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'project.member_set', 'project', ?, ?, ?)",
        )
        .bind(Uuid::new_v4().to_string())
        .bind(workspace_id.to_string())
        .bind(format!("human:{actor_user_id}"))
        .bind(project_id.to_string())
        .bind(now.to_rfc3339())
        .bind(serde_json::json!({"user_id": target_user_id, "role": role.as_str()}).to_string())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(ProjectMembership {
            project_id,
            user_id: target_user_id,
            role,
            created_at: now,
        })
    }

    async fn revoke_project_member(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
        actor_user_id: Uuid,
        target_user_id: Uuid,
    ) -> Result<bool, StorageError> {
        let mut tx = self.pool.begin().await?;
        let project_exists: Option<i64> =
            sqlx::query_scalar("SELECT 1 FROM projects WHERE id = ? AND workspace_id = ?")
                .bind(project_id.to_string())
                .bind(workspace_id.to_string())
                .fetch_optional(&mut *tx)
                .await?;
        if project_exists.is_none() {
            return Err(StorageError::ProjectNotFound);
        }
        let actor_workspace_member: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM workspace_memberships WHERE workspace_id = ? AND user_id = ?",
        )
        .bind(workspace_id.to_string())
        .bind(actor_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if actor_workspace_member.is_none() {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let actor_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM project_memberships WHERE project_id = ? AND user_id = ? AND revoked_at IS NULL",
        )
        .bind(project_id.to_string())
        .bind(actor_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if !matches!(actor_role.as_deref(), Some("owner" | "admin")) {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let target_role: Option<String> = sqlx::query_scalar(
            "SELECT role FROM project_memberships WHERE project_id = ? AND user_id = ? AND revoked_at IS NULL",
        )
        .bind(project_id.to_string())
        .bind(target_user_id.to_string())
        .fetch_optional(&mut *tx)
        .await?;
        if target_role.as_deref() == Some("owner")
            || (actor_role.as_deref() == Some("admin") && target_role.as_deref() == Some("admin"))
        {
            return Err(StorageError::ProjectAuthorizationDenied);
        }
        let now = Utc::now().to_rfc3339();
        let changed = sqlx::query(
            "UPDATE project_memberships SET revoked_at = ? WHERE project_id = ? AND user_id = ? AND revoked_at IS NULL AND role != 'owner'",
        )
        .bind(&now)
        .bind(project_id.to_string())
        .bind(target_user_id.to_string())
        .execute(&mut *tx)
        .await?;
        if changed.rows_affected() == 1 {
            sqlx::query(
                "INSERT INTO audit_events (event_id, workspace_id, actor, event_type, entity_type, entity_id, occurred_at, metadata_json) VALUES (?, ?, ?, 'project.member_revoked', 'project', ?, ?, ?)",
            )
            .bind(Uuid::new_v4().to_string())
            .bind(workspace_id.to_string())
            .bind(format!("human:{actor_user_id}"))
            .bind(project_id.to_string())
            .bind(&now)
            .bind(serde_json::json!({"user_id": target_user_id}).to_string())
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(changed.rows_affected() == 1)
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
