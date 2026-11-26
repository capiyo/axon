# Axon Memory Implementation Audit

> Audit date: 2026-11-26  
> Repository: `capiyo/axon`  
> Branch: `production/phase-b-chats-memory`  
> Status: implementation in progress; compiler/CI verification is still required.

## 1. Verified architecture

- Rust workspace with Axum backend at `backend/axum`.
- SQLite remains the authoritative relational store through `Storage` and `SqliteStorage`.
- MongoDB is isolated behind `MemoryDocumentRepository` and stores immutable versioned memory documents.
- SQLite stores document lifecycle, scope, version, content hash, source sequence, synchronization state, audit data, and transactional outbox records.
- A leased outbox worker persists MongoDB versions, verifies hashes, retries failures, supports dead-letter state and restart recovery, and exposes audited retry for valid dead-letter payloads.
- Existing chat memory, Firebase authentication, workspace membership, Qoin market-vector adapter, memory intelligence, QKV attention, immutable epochs, and progressive agent-memory sessions remain in place.

## 2. Existing QKV and Qoin implementation

- `memory_architecture.rs` implements multi-head scaled dot-product attention, Pareto handover selection, immutable epoch publication, agent memory sessions, and progressive retrieval.
- `memory_vector_provider.rs` implements deterministic FNV-1a lexical feature hashing (64 dimensions). This is a deterministic lexical representation, not a neural embedding model.
- `memory_intelligence.rs` implements deterministic Qoin memory ranking with semantic, context, dependency, outcome, failure, evidence, recency, and tag features, plus score explanations. A versioned `QoinScoringConfig` validates weights and thresholds, is configurable at startup, and is returned with search responses.
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
2. Added session-bound hydration that resolves the epoch from `agent_memory_sessions`; callers cannot select an arbitrary epoch.
3. Added authenticated session-document retrieval and a combined session-context endpoint that returns progressive SQLite retrieval and MongoDB documents pinned to the same session epoch.
4. Enforced workspace membership for document approval and retrieval, plus agent/session association, task assignment, pinned visibility scope, authoritative SQLite metadata, MongoDB version identity, QKV version, and SHA-256 integrity before returning document values. Agent identity is still path-selected rather than cryptographically authenticated; see remaining security gaps.
5. Bounded serialized MongoDB session document context to 256 KiB and complete serialized QKV/Qoin search results to 64 KiB; whole documents/results are omitted rather than truncated, and responses report truncation.
6. New document creation persists deterministic lexical QKV keys and model versions in MongoDB `key_metadata`.
7. Added SHA-256 source hashing over the exact ordered title/content/tag text used to build each persisted QKV key.
8. Document validation rejects malformed, partial, non-normalized, model-version-mismatched, or source-hash-mismatched persisted keys. Legacy documents without stored keys retain deterministic query-time fallback.
9. Added tests for exact epoch hydration, session binding, private-scope denial, missing epochs, persisted QKV-key validation, and context-budget selection.
10. Added validated startup configuration for production-required MongoDB, connection/server-selection timeouts, maximum pool size, URI-redacted Debug output, and a pluggable asynchronous QKV provider injected through AppState.
11. Added configurable, versioned Qoin scoring policies and returned the exact policy with ranked search results.
12. Made outbox completion and failure transitions verify their lease and authoritative metadata row before committing.
13. Added audited dead-letter retry for valid retained payloads, with SQLite-only transaction semantics and tests for invalid payload refusal.
14. Added bounded, workspace-scoped, read-only SQLite/MongoDB reconciliation and an authenticated report endpoint; no automatic repair is performed.
15. Added `/ready` to report SQLite readiness and optional/configured MongoDB readiness separately from `/health` liveness.
16. Added tests for config policy validation, scoring-weight changes, reconciliation mismatches, dead-letter recovery, and public readiness.
17. Updated architecture, QKV, MongoDB, testing, security, operations, quickstart, data-model, and repository-state documentation. Added bounded graceful-shutdown waiting for the outbox worker.

## 5. Remaining gaps

- A production neural embedding-provider implementation and configuration. The asynchronous `MemoryVectorProvider` abstraction is injected and ready for an implementation; only the deterministic lexical provider is currently shipped.
- Project-shared memory remains fail-closed because persisted project-membership authorization does not yet exist.
- A separately searchable precomputed vector index and safe index-rebuild workflow. A bounded read-only reconciliation report now exists, but it does not repair data.
- Retrieval feedback persistence and evaluation against a labelled memory-retrieval dataset.
- Full MongoDB outage/restart integration tests.
- Full readiness/observability coverage (basic DB readiness exists, worker health/metrics do not), backup/restore drills, production secrets management, and measured performance results.
- Cryptographic agent-principal authentication is not implemented. Current agent IDs are selected in authenticated workspace-member routes; a workspace member can request context for another registered agent. Agent-private scope is therefore agent-ID isolation, not a security boundary against other members of the same workspace. Do not treat it as production-grade isolation until agent credentials/principal authorization are implemented.
- Session-document hydration is now bound to the persisted session epoch, and a combined session-context API returns it alongside progressive retrieval. The repository still has no autonomous provider-backed agent execution runtime to wire this context into; client/agent consumer integration needs to use the new endpoint.

## 6. Verification status

The GitHub connector in this session can read and commit repository files but does not provide a Rust build/test runtime. No `cargo fmt`, `cargo check`, `cargo test`, or `cargo clippy` pass is claimed.

Run from repository root in a Rust-enabled checkout:

```sh
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

A MongoDB-enabled integration run must also exercise outbox retries/restart, hash conflicts, exact session epoch hydration, workspace/task/private scope isolation, and reconciliation against real MongoDB.

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
