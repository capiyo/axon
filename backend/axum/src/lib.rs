pub mod agent_runtime;
pub mod ai_provider;
pub mod audit;
pub mod auth_middleware;
#[cfg(test)]
mod auth_middleware_tests;
pub mod capability;
pub mod chat_memory;
pub mod config;
pub mod explore;
pub mod firebase_auth;
pub mod identity;
pub mod memory;
pub mod memory_architecture;
pub mod memory_documents;
pub mod memory_intelligence;
pub mod memory_outbox;
pub mod memory_store;
pub mod memory_system;
pub mod memory_vector_provider;
pub mod model_discovery;
pub mod model_registry;
pub mod models;
pub mod qoin_vectors;
pub mod robotics;
pub mod routes;
pub mod storage;
pub mod supervisor;
pub mod supervisor_roles;
pub mod vector_decision;
pub(crate) mod handlers;

pub use state::AppState;
pub use routes::app;
mod state;
pub mod model { pub use crate::models::*; }
