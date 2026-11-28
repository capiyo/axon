include!("base.rs");

impl Storage for SqliteStorage {
    include!("core.rs");
    include!("identity.rs");
    include!("credentials.rs");
    include!("memberships.rs");
    include!("workspace_billing.rs");
}

#[cfg(test)]
include!("tests.rs");
