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

include!("types.rs");
include!("assignment.rs");
include!("persistence.rs");

#[cfg(test)]
include!("tests.rs");
