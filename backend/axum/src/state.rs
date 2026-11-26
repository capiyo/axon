use crate::{
    audit::{AuditEvent, AuditStore},
    chat_memory::ChatMemoryStore,
    firebase_auth::FirebaseVerifier,
    memory_architecture::MemoryEpochCoordinator,
    memory_documents::MemoryDocumentRepository,
    memory_outbox::MemoryOutbox,
    model::{Agent, Artifact, Project, ProjectMembership, ProjectRole, Task, Workspace},
    qoin_vectors::QoinVectorClient,
    memory_intelligence::QoinScoringConfig,
    memory_vector_provider::MemoryVectorProvider,
    robotics::TelemetryEvent,
    storage::{EntityKind, Storage, StorageError, StorageMutation, StoredEntity, UserWorkspace},
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
    /// Bounded in-memory simulator stream; never treated as durable device telemetry.
    pub telemetry: Arc<RwLock<TelemetryStore>>,
    /// Runtime cache backed by the append-only audit table when SQLite is configured.
    pub audit: Arc<RwLock<AuditStore>>,
    storage: Option<Arc<dyn Storage>>,
    pub chat_memory: Option<ChatMemoryStore>,
    pub memory_epochs: Option<MemoryEpochCoordinator>,
    /// Long-form document storage. Authorization and lifecycle remain in SQLite.
    pub memory_documents: Option<Arc<dyn MemoryDocumentRepository>>,
    pub memory_outbox: Option<MemoryOutbox>,
    pub qoin_vectors: Option<QoinVectorClient>,
    pub qoin_memory_scoring: QoinScoringConfig,
    pub memory_vector_provider: Option<Arc<dyn MemoryVectorProvider>>,
    pub firebase_verifier: Option<FirebaseVerifier>,
    pub simulator_enabled: bool,
}

impl AppState {
    pub async fn with_storage(storage: Arc<dyn Storage>) -> Result<Self, StorageError> {
        let chat_memory = Some(storage.chat_memory_store());
        let memory_epochs = Some(storage.memory_epoch_coordinator());
        let state = Self {
            storage: Some(storage.clone()),
            chat_memory,
            memory_epochs,
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

        let workspace_ids: Vec<_> = state.workspaces.read().await.keys().copied().collect();
        let mut audit_events = Vec::new();
        for workspace_id in workspace_ids {
            audit_events.extend(storage.list_audit_events(workspace_id).await?);
        }
        state.audit.write().await.restore(audit_events);

        Ok(state)
    }

    pub fn with_memory_document_repository(
        mut self,
        repository: Option<Arc<dyn MemoryDocumentRepository>>,
    ) -> Self {
        self.memory_documents = repository;
        self
    }

    pub fn with_memory_outbox(mut self, outbox: Option<MemoryOutbox>) -> Self {
        self.memory_outbox = outbox;
        self
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

    pub fn with_qoin_vector_client(mut self, client: Option<QoinVectorClient>) -> Self {
        self.qoin_vectors = client;
        self
    }

    pub fn with_qoin_memory_scoring_config(
        mut self,
        config: QoinScoringConfig,
    ) -> Self {
        if let Some(coordinator) = self.memory_epochs.take() {
            self.memory_epochs = Some(
                coordinator.with_qoin_scoring_version(config.effective_version()),
            );
        }
        self.qoin_memory_scoring = config;
        self
    }

    pub fn with_memory_vector_provider(
        mut self,
        provider: Option<Arc<dyn MemoryVectorProvider>>,
    ) -> Self {
        self.memory_vector_provider = provider;
        self
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

    pub async fn is_agent_owner(
        &self,
        user_id: Uuid,
        workspace_id: Uuid,
        agent_id: Uuid,
    ) -> bool {
        self.agents.read().await.get(&agent_id).is_some_and(|agent| {
            agent.workspace_id == workspace_id && agent.created_by_user_id == Some(user_id)
        })
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

    pub async fn create_project_for_user(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
        name: String,
    ) -> Result<Project, StorageError> {
        match &self.storage {
            Some(storage) => storage.create_project_for_user(workspace_id, user_id, name).await,
            None => Err(StorageError::IdentityStorageUnavailable),
        }
    }

    pub async fn list_user_projects(
        &self,
        workspace_id: Uuid,
        user_id: Uuid,
    ) -> Result<Vec<Project>, StorageError> {
        match &self.storage {
            Some(storage) => storage.list_user_projects(workspace_id, user_id).await,
            None => Err(StorageError::IdentityStorageUnavailable),
        }
    }

    pub async fn is_project_member(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
        user_id: Uuid,
    ) -> Result<bool, StorageError> {
        match &self.storage {
            Some(storage) => storage.is_project_member(workspace_id, project_id, user_id).await,
            None => Ok(false),
        }
    }

    pub async fn set_project_member(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
        actor_user_id: Uuid,
        target_user_id: Uuid,
        role: ProjectRole,
    ) -> Result<ProjectMembership, StorageError> {
        match &self.storage {
            Some(storage) => storage.set_project_member(
                workspace_id, project_id, actor_user_id, target_user_id, role,
            ).await,
            None => Err(StorageError::IdentityStorageUnavailable),
        }
    }

    pub async fn revoke_project_member(
        &self,
        workspace_id: Uuid,
        project_id: Uuid,
        actor_user_id: Uuid,
        target_user_id: Uuid,
    ) -> Result<bool, StorageError> {
        match &self.storage {
            Some(storage) => storage.revoke_project_member(
                workspace_id, project_id, actor_user_id, target_user_id,
            ).await,
            None => Err(StorageError::IdentityStorageUnavailable),
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

    pub fn storage_is_durable(&self) -> bool {
        self.storage.is_some()
    }

    pub async fn storage_is_ready(&self) -> bool {
        match &self.storage {
            Some(storage) => storage.ping().await.is_ok(),
            None => false,
        }
    }
}
