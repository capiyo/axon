# Axon Memory Implementation Audit

> Audit date: 2026-11-25  
> Branch inspected: `production/phase-b-chats-memory`  
> Repository inspected: `capiyo/axon`

## 1. Verified current architecture

- Rust workspace contains `backend/axum` as the `axon-api` package.
- The backend uses Axum 0.7, Tokio, SQLx 0.8, and SQLite.
- `Storage` is an asynchronous trait in `backend/axum/src/storage.rs`; `SqliteStorage` is its current durable implementation.
- SQLite migrations are embedded from `backend/axum/migrations` and applied by `SqliteStorage::connect`.
- `AppState` owns shared handles for the storage trait, chat-memory store, memory-epoch coordinator, optional Qoin vector client, and Firebase verifier.
- Startup in `backend/axum/src/main.rs` configures Firebase, SQLite, the hourly memory-epoch worker, and an optional Qoin HTTP client.

## 2. Existing memory and QKV/Qoin functionality

The repository already implements substantial parts of the requested intelligence architecture:

- `memory_architecture.rs` defines `QoinVector`, `AttentionQuery`, `AttentionMemory`, `AttentionScore`, multi-head scaled dot-product `qkv_attention`, and Pareto-style handover selection.
- `memory_architecture.rs` also coordinates immutable memory epochs, epoch items, agent memory sessions, handover, and progressive retrieval.
- `memory_intelligence.rs` contains a separate deterministic memory-ranking model with workspace/scope/status filtering, semantic similarity, context, dependency, outcome, failure, evidence, recency, tags, and score explanations.
- `qoin_vectors.rs` is an HTTP adapter to the separate Qoin service. It validates six-dimensional attended market vectors and supports historical event-time lookups. This market-vector contract must not be conflated with Axon's memory-ranking algorithm.
- `memory_routes.rs` exposes existing memory indexing, Qoin-vector, epoch, handover, and progressive retrieval routes.
- `chat_memory.rs` persists chats, messages, memory entries, and resume-brief cache data in SQLite.

These components must be extended rather than replaced or duplicated.

## 3. Existing storage responsibilities

The existing `Storage` trait owns entity persistence, audit events, Firebase user provisioning, workspace membership, workspace listing, and workspace creation. It also provides chat-memory and memory-epoch coordinator handles. The current concrete adapter is `SqliteStorage`.

The current `AppState` does not yet contain a MongoDB document repository.

## 4. Existing database schema

The migration history currently includes migrations `0001` through `0009`. The verified schema evolution includes:

- Core entities and append-only audit events.
- Users and workspace memberships.
- Chats, chat messages, memory events, and resume-brief cache.
- Memory epochs, vector index, and agent memory sessions.
- Immutable epoch-item snapshots.
- Progressive retrieval tracking.
- Snapshot immutability triggers.
- Chat-message source-kind support.
- Memory-index revision timestamp.

Existing vector-index rows already carry Qoin vector JSON, key metadata JSON, value payload JSON, tags, dependency IDs, outcome/evidence scores, source sequence, model version, and indexing timestamp.

## 5. Configuration and startup

- SQLite is configured by `AXON_DATABASE_URL`, defaulting to `sqlite://axon.db`.
- Firebase authentication is required by current startup.
- The external Qoin HTTP service is optional and configured by `QOIN_API_URL`.
- MongoDB is not currently declared in the inspected Cargo manifests and is not initialized by startup.

## 6. Gaps to address

1. No MongoDB document repository exists.
2. No versioned long-form memory-document model is present in the inspected storage/state layer.
3. No SQLite transactional outbox for cross-database document persistence was found in the inspected implementation.
4. Existing QKV/Qoin retrieval currently hydrates value payloads from SQLite epoch snapshots; it does not yet hydrate versioned documents from MongoDB.
5. Existing epoch items pin vector and value payload snapshots, but the inspected schema does not establish MongoDB document-version/hash pinning.
6. Existing authorization protects workspace membership and memory visibility in the current routes/coordinator. New document operations must reuse and strengthen these checks rather than bypass them.
7. The external Qoin market-vector service is distinct from the deterministic Axon memory scoring model and must remain isolated.

## 7. Compatibility risks

- Adding MongoDB as a mandatory startup dependency without an explicit deployment configuration would break current SQLite-only development and deployments.
- Changing `Storage` directly to embed Mongo-specific methods would couple the relational adapter to document storage.
- Replacing existing QKV or memory-intelligence code would regress established behavior and tests.
- Reusing the market Qoin vector endpoint as a memory embedding provider would mix two different domains and vector contracts.
- Existing immutable epochs must not be rewritten by later document or index updates.

## 8. Safe implementation sequence

1. Introduce a separate `MemoryDocumentRepository` trait and MongoDB adapter.
2. Add explicit MongoDB configuration and shared application-state wiring while preserving SQLite-only mode.
3. Add a versioned memory-document model with deterministic content hashes.
4. Add forward-only SQLite migrations for document metadata and transactional outbox state.
5. Implement an idempotent outbox worker and crash recovery.
6. Connect the existing QKV/Qoin indexing pipeline to ready document versions without replacing its scoring behavior.
7. Extend immutable epoch pins with document version/hash and index versions.
8. Expose authenticated document-memory operations through existing Axum conventions.
9. Add integration/security tests and operational documentation.

## 9. Files inspected

- `Cargo.toml`
- `backend/axum/Cargo.toml`
- `backend/axum/src/storage.rs`
- `backend/axum/src/state.rs`
- `backend/axum/src/main.rs`
- `backend/axum/src/lib.rs`
- `backend/axum/src/memory_architecture.rs`
- `backend/axum/src/memory_intelligence.rs`
- `backend/axum/src/qoin_vectors.rs`
- `backend/axum/src/chat_memory.rs`
- `backend/axum/src/memory_routes.rs`

## 10. Verification limitation

This audit was performed against GitHub source files. The connected GitHub workflow permits repository reads and writes but does not provide a local checkout or Rust execution environment in this session. Consequently, compilation, migration execution, and integration tests must be run in CI or a local checkout before any implementation can be represented as verified.
