use crate::{
    audit::AuditEvent,
    model_registry::{ModelChangeFlag, ModelProvider, ModelRecord, RegistryStatus},
    storage::{EntityKind, Storage, StorageMutation, StoredEntity},
};
use async_trait::async_trait;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
    time::Duration,
};
use thiserror::Error;
use uuid::Uuid;

#[derive(Debug, Clone, Deserialize, Serialize)]

include!("model_discovery/catalog.rs");
include!("model_discovery/refresh.rs");
include!("model_discovery/scheduler.rs");

#[cfg(test)]
include!("model_discovery/tests.rs");
