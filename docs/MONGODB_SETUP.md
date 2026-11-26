# MongoDB Memory Document Setup

MongoDB is a separate document repository. SQLite remains authoritative for Axon identity, workspace membership, document-version metadata, lifecycle, and outbox state.

## Environment

| Variable | Purpose | Default |
|---|---|---|
| `AXON_MONGODB_URI` | MongoDB connection URI | unset |
| `AXON_MONGODB_DATABASE` | Database name | `axon` |
| `AXON_MONGODB_COLLECTION` | Memory document collection | `memory_documents` |
| `AXON_MEMORY_DOCUMENTS_REQUIRED` | Require MongoDB outside production | `false`; production always requires MongoDB |
| `AXON_MONGODB_CONNECT_TIMEOUT_MS` | Driver connection timeout | `10000` (100–120000) |
| `AXON_MONGODB_SERVER_SELECTION_TIMEOUT_MS` | Server selection/ping timeout | `5000` (100–120000) |
| `AXON_MONGODB_MAX_POOL_SIZE` | Maximum pooled connections | `20` (1–500) |
| `AXON_QOIN_MEMORY_SCORING_CONFIG` | Optional JSON versioned Qoin memory scoring policy | built-in `axon-qoin-memory-v1` policy |
| `AXON_ENV` | Runtime environment | unset |

Local SQLite-only mode remains available when MongoDB is not configured. Production mode always requires MongoDB; `AXON_MEMORY_DOCUMENTS_REQUIRED=false` cannot disable that requirement. Non-production and test environments can use SQLite-only mode.

## Startup behavior

The API creates one MongoDB client, pings the selected database, and creates:
- A unique compound index on `memory_id + version`.
- A workspace/scope/status lookup index.

The MongoDB URI is never intentionally logged. Connection timeout and pool size are bounded and configurable. Startup errors intentionally avoid echoing the URI. Configure credentials through the deployment secret manager, not source control.

## Document version rules

A document version is immutable. Replaying an identical version is idempotent. Reusing the same memory ID and version with different document data is rejected. Content integrity is checked using SHA-256 of UTF-8 content. Each new document is also tied to the SQLite audit sequence that records its creation.

SQLite stores the authoritative metadata and pending outbox payload. The outbox payload is cleared after successful MongoDB persistence. SQLite and MongoDB must both be backed up.

## Local example

Start a local MongoDB instance, then set:

```powershell
$env:AXON_MONGODB_URI="mongodb://127.0.0.1:27017"
$env:AXON_MONGODB_DATABASE="axon"
$env:AXON_MONGODB_COLLECTION="memory_documents"
$env:AXON_MEMORY_DOCUMENTS_REQUIRED="true"
```

Then run `cargo run -p axon-api`.

## Current implementation boundary

MongoDB connection lifecycle, immutable document writes, hash verification, and the SQLite outbox worker are implemented in this branch. Direct retrieval by memory ID/version is implemented after explicit SQLite approval and scope verification. Query-based search is available through deterministic lexical QKV attention followed by Qoin memory ranking. New immutable document versions include the precomputed lexical QKV key, its version, and a source-text SHA-256 integrity hash in `key_metadata`. The combined session-context endpoint returns progressive SQLite retrieval plus exact MongoDB versions pinned to the same agent session epoch. `GET /ready` probes SQLite and MongoDB readiness. The authenticated workspace reconciliation endpoint provides a bounded, read-only comparison of SQLite metadata and MongoDB document versions. Neural embedding-provider implementation, persisted vector index/rebuild, reconciliation, and MongoDB outage integration tests remain outstanding.
