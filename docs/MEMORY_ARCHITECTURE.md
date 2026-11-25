# Axon Memory Architecture

## Ownership boundaries

- **SQLite:** authoritative identity, workspace membership, memory metadata, lifecycle, document versions, source sequence, synchronization state, auditability, and transactional outbox.
- **MongoDB:** immutable versioned long-form memory documents.
- **QKV:** query/key/value representation and attention-based candidate selection.
- **Qoin memory intelligence:** deterministic memory relevance scoring and ranking explanations.
- **Axon memory services:** authorization, orchestration, epoch pinning, context budgets, and agent-facing access.

The separate Qoin HTTP vector adapter is for market/event-time vectors. It is not an interchangeable embedding provider for memory documents.

## Current data flow

1. An authenticated workspace member submits a memory document.
2. Axon validates the requested scope against the current workspace, task, or agent records.
3. SQLite atomically stores authoritative document metadata and a durable outbox payload.
4. The background worker claims the event using a lease.
5. The worker inserts the immutable document version in MongoDB.
6. The worker verifies the document hash and marks SQLite synchronization as stored.
7. The worker clears the outbox payload after successful delivery.
8. A workspace member explicitly approves the proposed version; SQLite records approval and an audit event atomically.
9. A registered agent can retrieve an approved version by ID after SQLite scope checks and MongoDB hash verification.

Retries use a unique memory/version/event key. Worker leases permit recovery after process restarts.

## Existing QKV and Qoin

Axon already has multi-head scaled dot-product QKV attention and Pareto-style initial/deferred handover in `memory_architecture.rs`. It also has deterministic score decomposition, scope/status filtering, and relevance explanations in `memory_intelligence.rs`.

Those systems remain in place. Direct document-by-ID retrieval is implemented with SQLite scope checks and MongoDB hash verification. Query-based QKV candidate search and epoch document-version/hash pins have not yet been added.

## Scope model

The document schema supports:
- `agent_private`
- `task_shared`
- `project_shared`
- `workspace_shared`

The current create endpoint enables workspace-shared, task-shared, and agent-private scopes. Project-shared creation is rejected because the current repository does not yet have a persisted project-membership authorization model. This is intentional fail-closed behavior.

## Production gates

Before claiming end-to-end multi-agent memory completion, implement and test:
- Query-based QKV candidate search over MongoDB documents.
- QKV key generation/indexing for document versions.
- Qoin memory-ranking integration for MongoDB candidates.
- Immutable epoch pins containing document version, content hash, QKV version, Qoin version, and source sequence.
- Reconciliation and safe index rebuild.
- Project membership authorization.
- End-to-end restart and cross-scope security tests.
