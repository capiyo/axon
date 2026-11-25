# Axon Memory Implementation Audit

> Audit date: 2026-11-25  
> Repository: `capiyo/axon`  
> Branch: `production/phase-b-chats-memory`  
> Status: implementation in progress; compiler/CI verification is still required.

## 1. Verified architecture

- Rust workspace with Axum backend at `backend/axum`.
- SQLite remains the authoritative relational store through `Storage` and `SqliteStorage`.
- MongoDB is isolated behind `MemoryDocumentRepository` and stores immutable versioned memory documents.
- SQLite stores document lifecycle, scope, version, content hash, source sequence, synchronization state, audit data, and transactional outbox records.
- A leased outbox worker persists MongoDB versions, verifies hashes, retries failures, and supports dead-letter state and restart recovery.
- Existing chat memory, Firebase authentication, workspace membership, Qoin market-vector adapter, memory intelligence, QKV attention, immutable epochs, and progressive agent-memory sessions remain in place.

## 2. Existing QKV and Qoin implementation

- `memory_architecture.rs` implements multi-head scaled dot-product attention, Pareto handover selection, immutable epoch publication, agent memory sessions, and progressive retrieval.
- `memory_vector_provider.rs` implements deterministic FNV-1a lexical feature hashing (64 dimensions). This is a deterministic lexical representation, not a neural embedding model.
- `memory_intelligence.rs` implements deterministic Qoin memory ranking with semantic, context, dependency, outcome, failure, evidence, recency, and tag features, plus score explanations.
- `qoin_vectors.rs` is a separate adapter for Qoin market/event-time vectors. Market vectors are not treated as interchangeable with memory embeddings.
- MongoDB document search filters candidate metadata in SQLite, hydrates and verifies document versions, applies QKV attention, then runs Qoin memory ranking.

## 3. Database and consistency model

### SQLite

Authoritative for identity, workspace membership, document lifecycle, scope, versions, audit/source sequence, synchronization status, outbox events, memory epochs, session-to-epoch association, and immutable epoch pins.

### MongoDB

Stores immutable long-form document versions. A unique `memory_id + version` index prevents duplicate version records. Content is verified using SHA-256.

### Outbox

The SQLite transaction records metadata and a durable outbox event. The worker claims a leased event, writes the immutable MongoDB version, verifies it, and marks the SQLite version synchronized. Retries are idempotent.

### Immutable epochs

`memory_document_epoch_items` pins exact document version, content hash, scope, project/task/owner identifiers, source sequence, QKV version, Qoin vector version, scoring version, and snapshot time.

## 4. Work completed in this implementation pass

1. Added exact-version document hydration from the SQLite immutable epoch snapshot.
2. Added session-bound hydration that resolves the epoch from `agent_memory_sessions`; the API caller cannot supply an arbitrary epoch.
3. Added authenticated endpoint:
   `GET /api/v1/workspaces/{workspace_id}/agents/{agent_id}/memory-sessions/{session_id}/documents`.
4. Enforced workspace membership, agent/session association, task assignment, pinned visibility scope, SQLite version metadata, MongoDB version identity, QKV version, and SHA-256 content integrity before returning document values.
5. New document creation now persists a deterministic lexical QKV key and its model version in MongoDB `key_metadata`.
6. Added a SHA-256 source hash over the exact ordered title/content/tag text used to build that QKV key.
7. Document validation rejects malformed, partial, non-normalized, model-version-mismatched, or source-hash-mismatched persisted keys. Legacy documents without stored keys retain deterministic query-time fallback.
8. Added test source for exact epoch hydration, session binding, private-scope denial, missing epochs, and persisted QKV-key validation.
9. Updated architecture, QKV, MongoDB, testing, and repository-state documentation.

## 5. Remaining gaps

- Neural embedding-provider implementation and configuration. The current provider is explicitly lexical.
- Project-shared memory remains fail-closed because persisted project-membership authorization does not yet exist.
- A separately searchable precomputed vector index, reconciliation, and safe index-rebuild workflow.
- Retrieval feedback persistence and evaluation against a labelled memory-retrieval dataset.
- Dead-letter operator recovery and full MongoDB outage/restart integration tests.
- Full readiness/observability coverage, backup/restore drills, production secrets management, and measured performance results.
- Session-document hydration is now bound to the persisted session epoch; full end-to-end agent execution consumption still needs integration testing.

## 6. Verification status

The GitHub connector in this session can read and commit repository files but does not provide a Rust build/test runtime. No `cargo fmt`, `cargo check`, `cargo test`, or `cargo clippy` pass is claimed.

Run from repository root in a Rust-enabled checkout:

```sh
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

A MongoDB-enabled integration run must also exercise outbox retries/restart, hash conflicts, exact session epoch hydration, and workspace/task/private scope isolation.

## 7. Files inspected

- `Cargo.toml`
- `backend/axum/Cargo.toml`
- `backend/axum/src/storage.rs`
- `backend/axum/src/state.rs`
- `backend/axum/src/main.rs`
- `backend/axum/src/lib.rs`
- `backend/axum/src/memory_architecture.rs`
- `backend/axum/src/memory_intelligence.rs`
- `backend/axum/src/memory_documents.rs`
- `backend/axum/src/memory_outbox.rs`
- `backend/axum/src/memory_vector_provider.rs`
- `backend/axum/src/memory_document_routes.rs`
- `backend/axum/src/qoin_vectors.rs`
- `backend/axum/src/chat_memory.rs`
- `backend/axum/src/memory_routes.rs`
- `docs/STATE_OF_REPO.md`
- `docs/MEMORY_ARCHITECTURE.md`
- `docs/QKV_RETRIEVAL.md`
- `docs/QOIN_MEMORY_INTELLIGENCE.md`
- `docs/MONGODB_SETUP.md`
- `docs/MEMORY_TESTING.md`
