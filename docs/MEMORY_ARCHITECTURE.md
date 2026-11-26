# Axon Memory Architecture

## Ownership boundaries

- **SQLite:** authoritative identity, workspace membership, memory metadata, lifecycle, document versions, source sequence, synchronization state, auditability, and transactional outbox.
- **MongoDB:** immutable versioned long-form memory documents.
- **QKV:** query/key/value representation and attention-based candidate selection.
- **Qoin memory intelligence:** deterministic memory relevance scoring and ranking explanations.
- **Axon memory services:** authorization, orchestration, epoch pinning, context budgets, and agent-facing access.

The separate Qoin HTTP vector adapter is for market/event-time vectors. It is not an interchangeable embedding provider for memory documents.

## Current data flow

1. An authenticated workspace member submits a memory document. Agent-private creation additionally verifies the registering user owns the target agent.
2. Axon validates the requested scope against the current workspace, task, or agent records.
3. SQLite appends a `memory_document.created` audit event, uses its sequence as the immutable source-event sequence, and atomically stores authoritative metadata plus the durable outbox payload.
4. The background worker claims the event using a lease.
5. The worker inserts the immutable document version in MongoDB.
6. The worker verifies the document hash and marks SQLite synchronization as stored.
7. The worker clears the outbox payload after successful delivery.
8. A workspace member explicitly approves the proposed version; SQLite records approval and an audit event atomically.
9. The Firebase user who registered an agent can retrieve or search its approved versions after SQLite scope checks and MongoDB hash verification; other workspace members cannot impersonate that agent through its path ID.

Retries use a unique memory/version/event key. Worker leases permit recovery after process restarts.

## Existing QKV and Qoin

Axon already has multi-head scaled dot-product QKV attention and Pareto-style initial/deferred handover in `memory_architecture.rs`. It also has deterministic score decomposition, scope/status filtering, and relevance explanations in `memory_intelligence.rs`.

Those systems remain in place. Direct document-by-ID retrieval is implemented with SQLite scope checks and MongoDB hash verification. Query-based search is available using deterministic lexical QKV attention followed by the existing Qoin memory-intelligence scorer. The current document key provider is deterministic lexical hashing, not a neural embedding. New document versions persist the lexical QKV key and source hash. Immutable epochs snapshot exact document version, content hash, scope, source sequence, QKV version, and Qoin scoring version. The authenticated session-document endpoint resolves the epoch from `agent_memory_sessions` (the caller cannot select another epoch), then hydrates only those pinned MongoDB versions after rechecking workspace, agent/task scope, SQLite metadata, and SHA-256 content integrity. The combined `POST .../memory-sessions/{session_id}/context` operation now returns progressive SQLite memories and the exact MongoDB versions pinned to that session. It runs QKV attention and Qoin ranking over the pinned documents using `query_text` for the MongoDB key provider (separate from the SQLite progressive vector when model versions differ), returns component explanations and the active scoring policy, and enforces a 256 KiB serialized context budget with an explicit truncation indicator.

## Document lifecycle

Archive, restore, and logical deletion create a new immutable document version through the SQLite-authoritative outbox. The prior MongoDB version is not rewritten. Archive and delete create lifecycle tombstones that immediately suppress older approved versions from normal search and from subsequently published epochs. Restore creates a new approved version and becomes searchable after its MongoDB projection is synchronized. Existing immutable session epochs continue to hydrate their exact historical version, subject to current authorization.

Logical deletion does not physically purge MongoDB content. Retention scheduling, legal-hold policy, and permanent purge remain separate operational work and must not be inferred from the delete endpoint.

## Scope model

The document schema supports:
- `agent_private`
- `task_shared`
- `project_shared`
- `workspace_shared`

Project membership is now persisted in SQLite. Projects are created by workspace owners; workspace owners can add/revoke workspace members; project owners/admins can add project members and viewers, while only project owners can grant elevated project roles. Project owners cannot be demoted or revoked through the membership API. Project-shared memory creation requires a project ID and active membership. Search, direct retrieval, version history, approval, and immutable session hydration re-check current project membership against SQLite. Epochs may pin project-scoped versions, but a revoked member cannot retrieve them. Scope promotion between existing documents is not yet implemented.

## Production gates

Before claiming end-to-end multi-agent memory completion, implement and test:
- A neural embedding-provider implementation behind the existing `MemoryVectorProvider` abstraction, with model/version compatibility checks.
- A separately searchable precomputed vector index and safe index rebuild.
- Explicit, audited repair workflows based on the new read-only reconciliation report.
- General-purpose persisted capability grants for all workspace resources; agent credentials are implemented for a fixed allowlist of agent memory routes, bound to one agent/workspace, with 90-day expiry, rotation, and revocation.
- Agent/client consumers that use the combined session-context endpoint; no provider-backed agent execution runtime exists yet.
- Real MongoDB outage, restart, and cross-scope security tests.
- Qoin source reliability, contradiction/duplication, reuse-frequency, and retrieval-feedback features backed by labelled evaluation data.
- Backup/restore drills, worker health metrics, and measured retrieval/indexing performance.
