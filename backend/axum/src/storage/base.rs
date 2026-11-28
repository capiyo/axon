use crate::{
    audit::AuditEvent,
    chat_memory::ChatMemoryStore,
    memory_architecture::MemoryEpochCoordinator,
    memory_outbox::MemoryOutbox,
    model::{
        Agent, Artifact, Project, ProjectMembership, ProjectRole, Task, Workspace,
        WorkspaceMembership,
    },
    model_registry::{
        AssignmentDecision, ModelChangeFlag, ModelEvaluation, ModelProvider, ModelRecord, ModelStat,
    },
    supervisor::SupervisorRun,
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
    #[error("Google identity subject is invalid")]
    IdentityProjectMismatch,
    #[error("project was not found in the requested workspace")]
    ProjectNotFound,
    #[error("project membership or role does not authorize this operation")]
    ProjectAuthorizationDenied,
    #[error("target user is not an active member of the project workspace")]
    ProjectWorkspaceMembershipRequired,
    #[error("model evaluation contains an invalid score or cost")]
    InvalidModelEvaluation,
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
    pub identity_subject: String,
    pub email: Option<String>,
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
    SupervisorRun,
    Provider,
    Model,
    ModelEvaluation,
    ModelStat,
    AssignmentDecision,
    ModelChangeFlag,
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
            Self::ModelChangeFlag => "model_change_flag",
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
    ModelChangeFlag(ModelChangeFlag),
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
            Self::ModelChangeFlag(_) => EntityKind::ModelChangeFlag,
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
            Self::ModelChangeFlag(record) => record.id,
        }
    }

    fn workspace_id(&self) -> Option<Uuid> {
        match self {
            Self::Workspace(_) => None,
            Self::Agent(record) => Some(record.workspace_id),
            Self::Artifact(record) => Some(record.workspace_id),
            Self::Task(record) => Some(record.workspace_id),
            Self::SupervisorRun(record) => Some(record.workspace_id),
            Self::Provider(_)
            | Self::Model(_)
            | Self::ModelEvaluation(_)
            | Self::ModelStat(_)
            | Self::AssignmentDecision(_)
            | Self::ModelChangeFlag(_) => None,
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
            Self::ModelChangeFlag(record) => record.detected_at,
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

    async fn claim_task_execution(
        &self,
        task_id: Uuid,
        owner_token: Uuid,
        lease_seconds: u64,
    ) -> Result<bool, StorageError>;

    async fn release_task_execution(
        &self,
        task_id: Uuid,
        owner_token: Uuid,
    ) -> Result<(), StorageError>;

    async fn list_entities(
        &self,
        kind: EntityKind,
        workspace_id: Option<Uuid>,
    ) -> Result<Vec<StoredEntity>, StorageError>;

    async fn list_audit_events(&self, workspace_id: Uuid) -> Result<Vec<AuditEvent>, StorageError>;

    async fn ensure_google_user(
        &self,
        subject: &str,
        email: &str,
        display_name: Option<&str>,
        avatar_url: Option<&str>,
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
    async fn daily_model_spend(&self, user_id: Uuid, usage_date: &str)
        -> Result<u64, StorageError>;
    async fn reserve_model_spend(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
        usage_date: &str,
        amount_micros: u64,
        daily_limit_micros: u64,
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
