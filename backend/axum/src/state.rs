use crate::{
    audit::{AuditEvent, AuditStore},
    chat_memory::ChatMemoryStore,
    firebase_auth::FirebaseVerifier,
    model::{Agent, Artifact, Task, Workspace},
    model_registry::{DeterministicModelAssigner, ModelAssigner},
    robotics::TelemetryEvent,
    storage::{EntityKind, Storage, StorageError, StorageMutation, StoredEntity, UserWorkspace},
    supervisor::SupervisorRun,
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};
use tokio::sync::RwLock;
use uuid::Uuid;

#[derive(Default)]
pub struct TelemetryStore {
    pub events_by_device: HashMap<String, Vec<TelemetryEvent>>,
    pub last_sequence_by_device: HashMap<String, u64>,
    pub event_ids: HashSet<String>,
}

#[derive(Clone, Default)]
pub struct AppState {
    pub workspaces: Arc<RwLock<HashMap<Uuid, Workspace>>>,
    pub agents: Arc<RwLock<HashMap<Uuid, Agent>>>,
    pub artifacts: Arc<RwLock<HashMap<Uuid, Artifact>>>,
    pub tasks: Arc<RwLock<HashMap<Uuid, Task>>>,
    pub supervisor_runs: Arc<RwLock<HashMap<Uuid, SupervisorRun>>>,
    pub supervisor_run_lock: Arc<tokio::sync::Mutex<()>>,
    /// Bounded in-memory simulator stream; never treated as durable device telemetry.
    pub telemetry: Arc<RwLock<TelemetryStore>>,
    /// Runtime cache backed by the append-only audit table when SQLite is configured.
    pub audit: Arc<RwLock<AuditStore>>,
    storage: Option<Arc<dyn Storage>>,
    pub chat_memory: Option<ChatMemoryStore>,
    pub memory_store: Option<crate::memory_store::MemoryStore>,
    pub firebase_verifier: Option<FirebaseVerifier>,
    pub simulator_enabled: bool,
    platform_admin_uids: Arc<HashSet<String>>,
    assignment_engine: Option<Arc<dyn ModelAssigner>>,
}

impl AppState {
    pub async fn with_storage(storage: Arc<dyn Storage>) -> Result<Self, StorageError> {
        let chat_memory = Some(storage.chat_memory_store());
        let memory_store = chat_memory
            .as_ref()
            .map(|store| crate::memory_store::MemoryStore::new(store.sqlite_pool()));
        let state = Self {
            storage: Some(storage.clone()),
            chat_memory,
            memory_store,
            firebase_verifier: None,
            simulator_enabled: false,
            ..Self::default()
        };

        for entity in storage.list_entities(EntityKind::Workspace, None).await? {
            match entity {
                StoredEntity::Workspace(item) => {
                    state.workspaces.write().await.insert(item.id, item);
                }
                _ => return Err(StorageError::InvalidStoredEntityKind),
            }
        }
        for entity in storage.list_entities(EntityKind::Agent, None).await? {
            match entity {
                StoredEntity::Agent(item) => {
                    state.agents.write().await.insert(item.id, item);
                }
                _ => return Err(StorageError::InvalidStoredEntityKind),
            }
        }
        for entity in storage.list_entities(EntityKind::Artifact, None).await? {
            match entity {
                StoredEntity::Artifact(item) => {
                    state.artifacts.write().await.insert(item.id, item);
                }
                _ => return Err(StorageError::InvalidStoredEntityKind),
            }
        }
        for entity in storage.list_entities(EntityKind::Task, None).await? {
            match entity {
                StoredEntity::Task(item) => {
                    state.tasks.write().await.insert(item.id, item);
                }
                _ => return Err(StorageError::InvalidStoredEntityKind),
            }
        }

        for entity in storage
            .list_entities(EntityKind::SupervisorRun, None)
            .await?
        {
            match entity {
                StoredEntity::SupervisorRun(item) => {
                    state.supervisor_runs.write().await.insert(item.id, item);
                }
                _ => return Err(StorageError::InvalidStoredEntityKind),
            }
        }

        let workspace_ids: Vec<_> = state.workspaces.read().await.keys().copied().collect();
        let mut audit_events = Vec::new();
        for workspace_id in workspace_ids {
            audit_events.extend(storage.list_audit_events(workspace_id).await?);
        }
        state.audit.write().await.restore(audit_events);

        Ok(state)
    }

    pub async fn persist_entity(
        &self,
        entity: StoredEntity,
        related_entities: Vec<StoredEntity>,
        event: AuditEvent,
    ) -> Result<(), StorageError> {
        if let Some(storage) = &self.storage {
            storage
                .apply(StorageMutation {
                    entity,
                    related_entities,
                    audit: event.clone(),
                })
                .await?;
        }

        self.audit.write().await.append_event(event);
        Ok(())
    }

    pub fn with_firebase_verifier(
        mut self,
        verifier: FirebaseVerifier,
        simulator_enabled: bool,
    ) -> Self {
        self.firebase_verifier = Some(verifier);
        self.simulator_enabled = simulator_enabled;
        self
    }

    pub async fn ensure_firebase_user(
        &self,
        project_id: &str,
        uid: &str,
    ) -> Result<UserWorkspace, StorageError> {
        match &self.storage {
            Some(storage) => storage.ensure_firebase_user(project_id, uid).await,
            None => Err(StorageError::IdentityStorageUnavailable),
        }
    }

    pub async fn is_workspace_member(
        &self,
        user_id: uuid::Uuid,
        workspace_id: uuid::Uuid,
    ) -> Result<bool, StorageError> {
        match &self.storage {
            Some(storage) => storage.is_workspace_member(user_id, workspace_id).await,
            None => Ok(false),
        }
    }

    pub async fn list_user_workspaces(
        &self,
        user_id: uuid::Uuid,
    ) -> Result<Vec<Workspace>, StorageError> {
        match &self.storage {
            Some(storage) => storage.list_user_workspaces(user_id).await,
            None => Ok(Vec::new()),
        }
    }

    pub async fn create_workspace_for_user(
        &self,
        user_id: uuid::Uuid,
        name: String,
        actor: String,
    ) -> Result<Workspace, StorageError> {
        match &self.storage {
            Some(storage) => {
                storage
                    .create_workspace_for_user(user_id, name, actor)
                    .await
            }
            None => Err(StorageError::IdentityStorageUnavailable),
        }
    }

    pub async fn daily_model_spend(
        &self,
        user_id: Uuid,
        usage_date: &str,
    ) -> Result<u64, StorageError> {
        match &self.storage {
            Some(storage) => storage.daily_model_spend(user_id, usage_date).await,
            None => Ok(0),
        }
    }

    pub async fn reserve_model_spend(
        &self,
        user_id: Uuid,
        usage_date: &str,
        amount_micros: u64,
        daily_limit_micros: u64,
    ) -> Result<bool, StorageError> {
        match &self.storage {
            Some(storage) => {
                storage
                    .reserve_model_spend(user_id, usage_date, amount_micros, daily_limit_micros)
                    .await
            }
            None => Ok(false),
        }
    }

    pub async fn list_entities(
        &self,
        kind: EntityKind,
        workspace_id: Option<Uuid>,
    ) -> Result<Vec<StoredEntity>, StorageError> {
        match &self.storage {
            Some(storage) => storage.list_entities(kind, workspace_id).await,
            None => Ok(Vec::new()),
        }
    }

    pub async fn list_audit_events(
        &self,
        workspace_id: uuid::Uuid,
    ) -> Result<Vec<AuditEvent>, StorageError> {
        match &self.storage {
            Some(storage) => storage.list_audit_events(workspace_id).await,
            None => Ok(self.audit.read().await.list_workspace(workspace_id)),
        }
    }

    pub fn with_assignment_engine(mut self, engine: Arc<dyn ModelAssigner>) -> Self {
        self.assignment_engine = Some(engine);
        self
    }

    pub fn assignment_engine(&self) -> Arc<dyn ModelAssigner> {
        self.assignment_engine
            .clone()
            .unwrap_or_else(|| Arc::new(DeterministicModelAssigner))
    }

    pub fn with_platform_admin_uids(mut self, firebase_uids: Vec<String>) -> Self {
        self.platform_admin_uids = Arc::new(firebase_uids.into_iter().collect());
        self
    }

    pub async fn bootstrap_platform_admin(
        &self,
        user_id: Uuid,
        firebase_uid: &str,
    ) -> Result<bool, StorageError> {
        if !self.platform_admin_uids.contains(firebase_uid) {
            return Ok(false);
        }
        let Some(storage) = &self.storage else {
            return Ok(false);
        };
        if !storage.is_platform_admin(user_id).await? {
            storage.grant_platform_admin(user_id, firebase_uid).await?;
        }
        Ok(true)
    }

    pub fn storage_is_durable(&self) -> bool {
        self.storage.is_some()
    }
}
