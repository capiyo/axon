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
- The current implementation does not yet provide a supported operator retry/reconciliation command for dead-letter events.

## Backups

Back up SQLite with a consistent SQLite backup procedure and MongoDB using its supported backup tooling. Test restoration regularly. A backup of only one database is insufficient for full memory recovery.
