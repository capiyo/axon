use crate::{
    audit::AuditEvent,
    storage::{EntityKind, Storage, StorageError, StorageMutation, StoredEntity},
};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};
use thiserror::Error;
use uuid::Uuid;

include!("model_registry/types.rs");
include!("model_registry/assignment.rs");
include!("model_registry/persistence.rs");

#[cfg(test)]
include!("model_registry/tests.rs");
