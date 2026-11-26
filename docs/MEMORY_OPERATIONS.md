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
