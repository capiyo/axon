# Axon Memory Data Model

## SQLite: authoritative metadata

### `memory_document_versions`

Primary key: `(memory_id, version)`.

Stores:
- Workspace, project, task, and owner-agent references.
- Visibility scope.
- Document type.
- Content hash.
- Source audit-event sequence.
- QKV and Qoin vector version identifiers.
- Lifecycle status.
- MongoDB synchronization status.
- Created/updated timestamps.

Lifecycle and synchronization are independent. A document may be approved while still pending MongoDB persistence; it is not retrievable until synchronization is stored or indexed.

### `memory_document_outbox`

Stores a durable event with:
- Event ID.
- Memory ID and version.
- Event type.
- Temporary serialized document payload.
- Pending/processing/completed/dead-letter state.
- Attempt count.
- Availability timestamp.
- Lease owner and expiry.
- Sanitized last error.
- Creation/update timestamps.

The unique key `(memory_id, version, event_type)` makes retries idempotent. The payload is cleared after successful MongoDB persistence.

### `memory_document_epoch_items`

Primary key: `(epoch_id, memory_id, document_version)`.

Pins:
- Exact document version.
- Content hash.
- Scope and owner/task/project references.
- Source audit-event sequence.
- QKV version.
- Qoin vector/scoring version.
- Snapshot timestamp.

Update and delete triggers enforce immutability.

## MongoDB: immutable document versions

Collection documents include:
- Stable memory and workspace identifiers.
- Project/task/owner identifiers.
- Scope and document type.
- Version.
- Title, content, and content format.
- Tags and source provenance.
- Source event sequence.
- Created/updated timestamps.
- SHA-256 content hash.
- Key metadata and value payload.
- QKV and Qoin vector version metadata.
- Proposed/ready/archive view status and retention policy.

The unique compound MongoDB index on `memory_id + version` prevents two different records from occupying the same immutable version.

## QKV key representation

The current provider is `LexicalMemoryVectorProvider`:
- Version: `axon-lexical-qkv-v1`.
- Dimension: 64.
- Algorithm: stable FNV-1a token feature hashing.
- Normalization: L2.
- No network model or training call is made.

It is explicitly a lexical fallback, not a neural embedding. The asynchronous `MemoryVectorProvider` trait is injected through application state and is the extension point for remote or local embedding providers. The configured default in this branch is still lexical; no neural provider is shipped.

## Qoin ranking

Memory scoring default version: `axon-qoin-memory-v1`. The full scoring policy (weights, thresholds, recency half-life, and explanation thresholds) is validated at startup and returned in document-search responses. `AXON_QOIN_MEMORY_SCORING_CONFIG` may override the default with a versioned JSON policy.

The existing memory-intelligence scorer receives the same lexical vector used by QKV plus context, outcome, failure, evidence, tags, and dependency metadata. The response includes the component score breakdown and QKV attention score.

## Scope invariants

- `workspace_shared`: no narrower owner/task/project identifier.
- `agent_private`: requires an owner agent ID.
- `task_shared`: requires a task ID.
- `project_shared`: requires a project ID, but API creation and retrieval remain disabled until project membership authorization is implemented.


## Agent ownership

The existing serialized `Agent` entity now carries optional `created_by_user_id` metadata. New agents record the authenticated Firebase user who registered them. Memory, handover, and session routes require that owner; older agent entities without this field fail closed until an explicit ownership backfill. This is user ownership/delegation, not an independent cryptographic agent credential.
