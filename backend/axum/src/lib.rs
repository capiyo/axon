#[path = "agents/runtime.rs"]
pub mod agent_runtime;
#[path = "providers/ai.rs"]
pub mod ai_provider;
#[path = "observability/audit.rs"]
pub mod audit;
#[path = "auth/middleware.rs"]
pub mod auth_middleware;
#[cfg(test)]
#[path = "auth/middleware_tests.rs"]
mod auth_middleware_tests;
#[path = "capabilities/domain.rs"]
pub mod capability;
pub mod chat_memory;
#[path = "config/mod.rs"]
pub mod config;
pub mod explore;
#[path = "auth/firebase.rs"]
#[path = "auth/google.rs"]
pub mod google_auth;
#[path = "auth/identity.rs"]
pub mod identity;
#[path = "memory/repository.rs"]
pub mod memory;
pub mod memory_architecture;
pub mod memory_documents;
pub mod memory_intelligence;
pub mod memory_outbox;
#[path = "memory/store.rs"]
pub mod memory_store;
#[path = "memory/system.rs"]
pub mod memory_system;
#[path = "memory/vector_provider.rs"]
pub mod memory_vector_provider;
pub mod model_discovery;
pub mod model_registry;
pub mod models;
#[path = "vectors/qoin.rs"]
pub mod qoin_vectors;
pub mod robotics;
pub mod routes;
pub mod storage;
#[path = "supervisor/domain.rs"]
pub mod supervisor;
#[path = "supervisor/roles.rs"]
pub mod supervisor_roles;
#[path = "vectors/decision.rs"]
pub mod vector_decision;
pub(crate) mod handlers;

#[cfg(test)]
#[path = "model_registry/routes_tests.rs"]
mod model_registry_routes_tests;
#[cfg(test)]
#[path = "supervisor/run_tests.rs"]
mod supervisor_run_tests;

pub use state::AppState;
pub use routes::app;
#[path = "app/state.rs"]
mod state;
pub mod model { pub use crate::models::*; }
