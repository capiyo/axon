//! Versioned MongoDB repository for long-form Axon memory documents.
//!
//! SQLite remains authoritative for identity, authorization, lifecycle, and
//! synchronization state. This module stores immutable document versions only;
//! it deliberately does not make authorization decisions.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::TryStreamExt;
use mongodb::{
    bson::doc,
    options::IndexOptions,
    Client, Collection, Database, IndexModel,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemoryDocument {
    pub memory_id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Option<Uuid>,
    pub task_id: Option<Uuid>,
    pub owner_id: Option<Uuid>,
    /// One of agent_private, task_shared, project_shared, workspace_shared.
    pub scope: String,
    pub document_type: String,
    pub version: u64,
    pub title: String,
    pub content: String,
    pub content_format: String,
    pub tags: Vec<String>,
    pub source: String,
    pub source_kind: String,
    pub source_event_sequence: u64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Lowercase hexadecimal SHA-256 of the UTF-8 content bytes.
    pub content_hash: String,
    pub key_metadata: Value,
    pub value_payload: Value,
    pub qkv_version: String,
    pub qoin_vector_version: Option<String>,
    /// proposed, ready, archived. Authorization/lifecycle authority remains SQLite.
    pub status: String,
    pub retention_policy: Option<String>,
}

impl MemoryDocument {
    pub fn calculate_content_hash(&self) -> String {
        let digest = Sha256::digest(self.content.as_bytes());
        format!("{digest:x}")
    }

    pub fn validate(&self) -> Result<(), MemoryDocumentError> {
        if self.version == 0
            || self.scope.trim().is_empty()
            || self.document_type.trim().is_empty()
            || self.title.trim().is_empty()
            || self.content_format.trim().is_empty()
            || self.source_kind.trim().is_empty()
            || self.qkv_version.trim().is_empty()
        {
            return Err(MemoryDocumentError::InvalidDocument);
        }
        if !matches!(
            self.scope.as_str(),
            "agent_private" | "task_shared" | "project_shared" | "workspace_shared"
        ) {
            return Err(MemoryDocumentError::InvalidDocument);
        }
        if !matches!(self.status.as_str(), "proposed" | "ready" | "archived") {
            return Err(MemoryDocumentError::InvalidDocument);
        }
        if self.content_hash != self.calculate_content_hash() {
            return Err(MemoryDocumentError::ContentHashMismatch);
        }
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum MemoryDocumentError {
    #[error("MongoDB operation failed: {0}")]
    Mongo(#[from] mongodb::error::Error),
    #[error("memory document is invalid")]
    InvalidDocument,
    #[error("memory document content hash does not match its content")]
    ContentHashMismatch,
    #[error("an immutable memory version already exists with different content")]
    VersionConflict,
}

#[async_trait]
pub trait MemoryDocumentRepository: Send + Sync {
    /// Insert an immutable version. Replaying the same version and hash is
    /// idempotent; attempting to reuse a version with different content fails.
    async fn put_version(&self, document: MemoryDocument) -> Result<(), MemoryDocumentError>;

    async fn get_version(
        &self,
        memory_id: Uuid,
        version: u64,
    ) -> Result<Option<MemoryDocument>, MemoryDocumentError>;

    async fn list_versions(
        &self,
        memory_id: Uuid,
    ) -> Result<Vec<MemoryDocument>, MemoryDocumentError>;

    async fn verify_version(
        &self,
        memory_id: Uuid,
        version: u64,
        expected_hash: &str,
    ) -> Result<bool, MemoryDocumentError>;

    async fn ping(&self) -> Result<(), MemoryDocumentError>;
}

#[derive(Clone)]
pub struct MongoMemoryDocumentRepository {
    _client: Client,
    collection: Collection<MemoryDocument>,
    database: Database,
}

impl MongoMemoryDocumentRepository {
    pub async fn connect(
        uri: &str,
        database_name: &str,
        collection_name: &str,
    ) -> Result<Self, MemoryDocumentError> {
        if uri.trim().is_empty()
            || database_name.trim().is_empty()
            || collection_name.trim().is_empty()
        {
            return Err(MemoryDocumentError::InvalidDocument);
        }

        let client = Client::with_uri_str(uri).await?;
        let database = client.database(database_name);
        database.run_command(doc! { "ping": 1 }, None).await?;
        let collection = database.collection::<MemoryDocument>(collection_name);

        let unique_version = IndexModel::builder()
            .keys(doc! { "memory_id": 1, "version": 1 })
            .options(IndexOptions::builder().unique(true).build())
            .build();
        let workspace_scope = IndexModel::builder()
            .keys(doc! { "workspace_id": 1, "scope": 1, "status": 1 })
            .build();
        collection.create_index(unique_version, None).await?;
        collection.create_index(workspace_scope, None).await?;

        Ok(Self {
            _client: client,
            collection,
            database,
        })
    }
}

#[async_trait]
impl MemoryDocumentRepository for MongoMemoryDocumentRepository {
    async fn put_version(&self, document: MemoryDocument) -> Result<(), MemoryDocumentError> {
        document.validate()?;
        let filter = doc! {
            "memory_id": document.memory_id.to_string(),
            "version": i64::try_from(document.version)
                .map_err(|_| MemoryDocumentError::InvalidDocument)?,
        };

        if let Some(existing) = self.collection.find_one(filter.clone(), None).await? {
            if existing == document {
                return Ok(());
            }
            return Err(MemoryDocumentError::VersionConflict);
        }

        match self.collection.insert_one(document.clone(), None).await {
            Ok(_) => Ok(()),
            Err(error) => {
                // A concurrent retry may have inserted the unique version first.
                // Treat it as success only when the persisted content hash agrees.
                if let Some(existing) = self.collection.find_one(filter, None).await? {
                    if existing == document {
                        return Ok(());
                    }
                    return Err(MemoryDocumentError::VersionConflict);
                }
                Err(MemoryDocumentError::Mongo(error))
            },
        }
    }

    async fn get_version(
        &self,
        memory_id: Uuid,
        version: u64,
    ) -> Result<Option<MemoryDocument>, MemoryDocumentError> {
        let version = i64::try_from(version).map_err(|_| MemoryDocumentError::InvalidDocument)?;
        Ok(self
            .collection
            .find_one(
                doc! { "memory_id": memory_id.to_string(), "version": version },
                None,
            )
            .await?)
    }

    async fn list_versions(
        &self,
        memory_id: Uuid,
    ) -> Result<Vec<MemoryDocument>, MemoryDocumentError> {
        let cursor = self
            .collection
            .find(doc! { "memory_id": memory_id.to_string() }, None)
            .await?;
        let mut versions: Vec<MemoryDocument> = cursor.try_collect().await?;
        versions.sort_by_key(|document| document.version);
        Ok(versions)
    }

    async fn verify_version(
        &self,
        memory_id: Uuid,
        version: u64,
        expected_hash: &str,
    ) -> Result<bool, MemoryDocumentError> {
        let document = self.get_version(memory_id, version).await?;
        match document {
            Some(document) => {
                document.validate()?;
                Ok(document.content_hash == expected_hash)
            }
            None => Ok(false),
        }
    }

    async fn ping(&self) -> Result<(), MemoryDocumentError> {
        self.database.run_command(doc! { "ping": 1 }, None).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_document() -> MemoryDocument {
        let mut document = MemoryDocument {
            memory_id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
            project_id: None,
            task_id: None,
            owner_id: None,
            scope: "workspace_shared".into(),
            document_type: "technical_decision".into(),
            version: 1,
            title: "Immutable snapshots".into(),
            content: "Epochs pin an exact memory document version.".into(),
            content_format: "text/markdown".into(),
            tags: vec!["memory".into(), "architecture".into()],
            source: "agent-session".into(),
            source_kind: "memory_entry".into(),
            source_event_sequence: 1,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            content_hash: String::new(),
            key_metadata: serde_json::json!({}),
            value_payload: serde_json::json!({}),
            qkv_version: "qkv-v1".into(),
            qoin_vector_version: None,
            status: "proposed".into(),
            retention_policy: None,
        };
        document.content_hash = document.calculate_content_hash();
        document
    }

    #[test]
    fn content_hash_is_stable_and_validated() {
        let document = sample_document();
        assert!(document.validate().is_ok());
        assert_eq!(document.content_hash, document.calculate_content_hash());
    }

    #[test]
    fn modified_content_cannot_reuse_a_stale_hash() {
        let mut document = sample_document();
        document.content.push_str(" changed");
        assert!(matches!(
            document.validate(),
            Err(MemoryDocumentError::ContentHashMismatch)
        ));
    }

    #[test]
    fn private_scope_is_an_explicit_supported_scope() {
        let mut document = sample_document();
        document.scope = "agent_private".into();
        assert!(document.validate().is_ok());
    }

    #[test]
    fn unknown_scope_is_rejected() {
        let mut document = sample_document();
        document.scope = "everyone".into();
        assert!(matches!(
            document.validate(),
            Err(MemoryDocumentError::InvalidDocument)
        ));
    }
}
