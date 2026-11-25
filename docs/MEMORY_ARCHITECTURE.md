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
3. SQLite appends a `memory_document.created` audit event, uses its sequence as the immutable source-event sequence, and atomically stores authoritative metadata plus the durable outbox payload.
4. The background worker claims the event using a lease.
5. The worker inserts the immutable document version in MongoDB.
6. The worker verifies the document hash and marks SQLite synchronization as stored.
7. The worker clears the outbox payload after successful delivery.
8. A workspace member explicitly approves the proposed version; SQLite records approval and an audit event atomically.
9. A registered agent can retrieve or search approved versions after SQLite scope checks and MongoDB hash verification.

Retries use a unique memory/version/event key. Worker leases permit recovery after process restarts.

## Existing QKV and Qoin

Axon already has multi-head scaled dot-product QKV attention and Pareto-style initial/deferred handover in `memory_architecture.rs`. It also has deterministic score decomposition, scope/status filtering, and relevance explanations in `memory_intelligence.rs`.

Those systems remain in place. Direct document-by-ID retrieval is implemented with SQLite scope checks and MongoDB hash verification. Query-based search is available using deterministic lexical QKV attention followed by the existing Qoin memory-intelligence scorer. The current document key provider is deterministic lexical hashing, not a neural embedding. New document versions persist the lexical QKV key and source hash. Immutable epochs snapshot exact document version, content hash, scope, source sequence, QKV version, and Qoin scoring version. The authenticated session-document endpoint resolves the epoch from `agent_memory_sessions` (the caller cannot select another epoch), then hydrates only those pinned MongoDB versions after rechecking workspace, agent/task scope, SQLite metadata, and SHA-256 content integrity. The combined `POST .../memory-sessions/{session_id}/context` operation now returns progressive SQLite memories and pinned MongoDB documents together, with a 256 KiB document-context budget and an explicit truncation indicator.

## Scope model

The document schema supports:
- `agent_private`
- `task_shared`
- `project_shared`
- `workspace_shared`

The current create endpoint enables workspace-shared, task-shared, and agent-private scopes. Project-shared creation is rejected because the current repository does not yet have a persisted project-membership authorization model. This is intentional fail-closed behavior.

## Production gates

Before claiming end-to-end multi-agent memory completion, implement and test:
- Neural embedding-provider integration and QKV index versioning for MongoDB documents.
- A separately searchable vector index, reconciliation, and safe index rebuild.
- Project membership authorization.
- Agent/client consumers must call the combined session-context endpoint; no provider-backed agent execution runtime exists yet.
- Real MongoDB restart/security tests.
- Reconciliation and safe index rebuild.
- Project membership authorization.
- End-to-end restart and cross-scope security tests.
