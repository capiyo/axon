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
- `project_shared`: requires a project ID and active SQLite project membership; project membership is subordinate to active workspace membership.


## Agent ownership

The existing serialized `Agent` entity now carries optional `created_by_user_id` metadata. New agents record the authenticated Firebase user who registered them. Memory, handover, and session routes require that owner; older agent entities without this field fail closed until an explicit ownership backfill. This is user ownership/delegation, not an independent cryptographic agent credential.


## Version history API

The authenticated agent-scoped history endpoint returns only approved and synchronized versions visible under SQLite scope metadata. It paginates by a descending `before_version` cursor, hydrates each exact MongoDB version, and verifies content hash, scope, project/task/owner metadata, source sequence, and QKV/Qoin versions before returning metadata. Document content is not included in the history response.


## Projects and memberships

Migration `0013_projects_and_memberships.sql` adds:
- `projects`: workspace-bound project identity, name, creator, and creation time.
- `project_memberships`: project/user association, role (`owner`, `admin`, `member`, `viewer`), creation time, and revocation timestamp.

Project creation is restricted to workspace owners. Project members must be workspace members. Workspace membership revocation revokes project memberships in the same transaction. Project-scoped memory creation, search, direct retrieval, version history, approval, and session hydration check current active membership. Immutable epoch records retain their historical pins, but current authorization revocation takes precedence at read time.


## Immutable lifecycle versions

Archive, restore, and logical delete are version transitions, not in-place edits to MongoDB documents. Each transition writes a new `memory_document_versions` row and a matching outbox payload:
- Archive writes lifecycle `archived`.
- Restore from the latest archived version writes a new approved (`ready`) version.
- Delete writes lifecycle `deleted` as a tombstone.

Tombstones suppress older approved versions from normal search and from newly published epochs. Already-published epochs keep their exact historical version pins. MongoDB retains the immutable document versions; the delete endpoint is logical deletion, not physical purge. Retention/purge execution and legal-hold policy are not implemented.


## Agent credentials

Migration `0014_agent_credentials.sql` adds credential metadata scoped to one workspace and one agent. Only a SHA-256 hash of the high-entropy secret is stored. Credential records include owner, creation/expiry, revocation, and last-used timestamps. Secrets expire after 90 days. Rotation revokes the old credential and inserts the replacement in one SQLite transaction. Middleware accepts the credential only on the matching agent's memory routes; general workspace/resource capabilities are not yet backed by persisted grants.


## Retrieval feedback

Migration `0015_memory_retrieval_feedback.sql` adds feedback rows bound to a workspace, authenticated agent session, agent, memory identity, exact document version, actor, event type, timestamp, and bounded evidence JSON. Feedback is accepted only when that exact version is pinned in the session's immutable epoch and the actor is authorized for the agent and scope. Events are auditable and do not mutate memory documents or epoch items.

Qoin scoring v2 includes a configurable `feedback_weight` in its effective policy fingerprint. Helpful/selected/reused events contribute a smoothed positive outcome signal; irrelevant/contradicted/incorrect events contribute a smoothed negative/failure signal. Historical epochs pinned to the v1 default do not apply v2 feedback weighting.
