include!("storage/base.rs");

impl Storage for SqliteStorage {
    include!("storage/core.rs");
    include!("storage/identity.rs");
    include!("storage/credentials.rs");
    include!("storage/memberships.rs");
    include!("storage/workspace_billing.rs");
}

#[cfg(test)]
include!("storage/tests.rs");
