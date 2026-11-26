# Memory Operations

## Runtime services

Axon currently uses:
- SQLite for transactional state and the durable outbox.
- MongoDB for immutable memory document versions.
- A background outbox worker started by the Axum process when MongoDB is configured.
- The existing hourly immutable memory epoch worker.

## Recovery

1. Keep SQLite and MongoDB backups.
2. Restore SQLite first so identity, version metadata, and pending outbox events are available.
3. Restore MongoDB document versions.
4. Start Axon with MongoDB required.
5. Allow the outbox worker to resume pending and expired-lease events.
6. Verify document hashes before enabling document retrieval.

Do not delete SQLite outbox rows manually to resolve delivery failures.

## Outbox behavior

- Events are claimed with a 60-second lease.
- A worker restart makes expired processing events eligible again.
- Retries use bounded exponential backoff.
- Eight failed attempts move an event to `dead_letter`.
- Each document creation records an append-only SQLite audit event; its sequence is copied into document metadata and the MongoDB payload.
- A workspace-authenticated recovery endpoint is available at `POST /api/v1/workspaces/{workspace_id}/memory/documents/{memory_id}/{version}/retry-sync`. It only requeues a dead-letter event with a retained valid payload, resets its attempt counter, changes SQLite synchronization state back to pending, and records an audit event in one SQLite transaction. The current role model has no administrator capability, so this endpoint is currently available to workspace members; restrict it to a dedicated operator role when role-based permissions are added.

## Backups

Back up SQLite with a consistent SQLite backup procedure and MongoDB using its supported backup tooling. Test restoration regularly. A backup of only one database is insufficient for full memory recovery.


## Graceful shutdown

The API signals both the memory outbox worker and the periodic epoch publisher during graceful shutdown and waits up to five seconds for each to finish its in-flight operation. Unclaimed work remains durable in SQLite and is eligible for recovery on the next process start. If the worker does not stop within the bounded wait, the process logs a warning; it does not mark pending events complete.


## Readiness and reconciliation

- `GET /health` is the liveness probe; it does not prove database readiness.
- `GET /ready` checks SQLite and, when configured, MongoDB ping readiness. In explicit local SQLite-only mode, MongoDB is reported as not configured while overall readiness may still be true.
- `GET /api/v1/workspaces/{workspace_id}/memory/reconciliation?limit=1000` returns a bounded, workspace-scoped, read-only comparison of SQLite authority and MongoDB versions. It reports missing, orphaned, invalid, and metadata-mismatched records plus outbox queue counts. It never repairs records or returns document content. Review the report and use an explicit audited repair procedure; automatic repair is not implemented.


## Version history

`GET /api/v1/workspaces/{workspace_id}/agents/{agent_id}/memory-documents/{memory_id}/versions/history?limit=20&before_version=...` returns a bounded page of approved, synchronized version metadata. For task-shared memories, supply `task_id`; the API rechecks task workspace and agent assignment. It verifies each exact MongoDB version against SQLite before returning title/hash/provenance metadata, and does not return document content.


## Project membership

- `POST /api/v1/workspaces/{workspace_id}/members` adds an existing Axon user to a workspace. Only a workspace owner may perform this operation.
- `DELETE /api/v1/workspaces/{workspace_id}/members/{target_user_id}` revokes a workspace member. Workspace owners cannot be removed by this operation.
- `POST /api/v1/workspaces/{workspace_id}/projects` creates a project; only a workspace owner may create one.
- `GET /api/v1/workspaces/{workspace_id}/projects` lists projects visible to the caller.
- `POST /api/v1/workspaces/{workspace_id}/projects/{project_id}/members` grants a project role. Project owners/admins may grant member/viewer; only project owners may grant admin/owner.
- `DELETE /api/v1/workspaces/{workspace_id}/projects/{project_id}/members/{target_user_id}` revokes non-owner project membership.

The target user must already have an Axon account and be an active member of the workspace. Workspace revocation revokes non-owner project memberships and agent credentials for that workspace. Project-owner rows are retained for ownership continuity, but active workspace membership is required for all project access; re-adding the user restores the retained owner role.

## Memory lifecycle

- `POST /api/v1/workspaces/{workspace_id}/memory/documents/{memory_id}/{version}/archive` creates an archived version.
- `POST /api/v1/workspaces/{workspace_id}/memory/documents/{memory_id}/{version}/restore` creates a new approved version from the latest archived version.
- `DELETE /api/v1/workspaces/{workspace_id}/memory/documents/{memory_id}/{version}` creates a logical deletion tombstone.

These operations create new immutable versions through the SQLite outbox. They do not rewrite existing MongoDB versions or immutable epoch items. Tombstones suppress the memory from normal search and newly published epochs; existing sessions can still resolve their pinned historical version if the caller remains authorized. Logical deletion does not physically purge content from MongoDB. Do not treat it as a retention or legal-deletion workflow.


## Agent credentials

Credential issuance, rotation, and revocation are human-owner operations:
- `POST /api/v1/workspaces/{workspace_id}/agents/{agent_id}/credentials`
- `POST /api/v1/workspaces/{workspace_id}/agents/{agent_id}/credentials/{credential_id}/rotate`
- `DELETE /api/v1/workspaces/{workspace_id}/agents/{agent_id}/credentials/{credential_id}`

The issue/rotation response contains the bearer token once. Deliver it to the agent through a secure secret channel; do not put it in source control, logs, issue trackers, or ordinary telemetry. Axon stores only a SHA-256 hash. Credentials expire after 90 days. Rotation revokes the previous credential and creates its replacement atomically. A credential is accepted only on its bound agent's memory-document, memory-session, and memory-context routes; it cannot be used for another agent or for credential-management/workspace routes.


## Session retrieval feedback

`POST /api/v1/workspaces/{workspace_id}/agents/{agent_id}/memory-sessions/{session_id}/feedback` records one of: `retrieved`, `selected`, `ignored`, `reused`, `helpful`, `irrelevant`, `contradicted`, or `incorrect`. The exact memory ID/version must be pinned in the session's immutable epoch. Evidence JSON is limited to 4 KiB. Feedback is attributed to the authenticated human owner and agent session and written to SQLite with an audit event; agent bearer credentials are not accepted for quality feedback.

Qoin scoring v2 uses a configurable `feedback_weight` to blend smoothed positive (helpful/selected/reused) and negative (irrelevant/contradicted/incorrect) feedback signals into outcome/failure dimensions. The effective policy fingerprint includes this weight. Sessions pinned to the legacy v1 default do not apply the v2 feedback weighting.


All project membership operations are evaluated against the current SQLite workspace/project membership state; cached project identifiers or session IDs do not grant access.
