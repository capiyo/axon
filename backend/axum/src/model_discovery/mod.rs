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

include!("catalog.rs");
include!("refresh.rs");
include!("scheduler.rs");

#[cfg(test)]
include!("tests.rs");
