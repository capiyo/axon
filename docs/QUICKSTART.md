# Local Quickstart

This guide runs the current manual-collaboration prototype locally. It is not a production deployment guide.

## 1. Start the API

Set `FIREBASE_PROJECT_ID` to the same Firebase project used by the clients and configure a durable `AXON_DATABASE_URL` (the default is local SQLite):

```sh
# PowerShell
$env:FIREBASE_PROJECT_ID="your-axon-firebase-project-id"
$env:AXON_DATABASE_URL="sqlite://axon.db"
# Optional in local SQLite-only mode; required when AXON_ENV=production
$env:AXON_MONGODB_URI="mongodb://127.0.0.1:27017"
$env:AXON_MONGODB_DATABASE="axon"
$env:AXON_MONGODB_COLLECTION="memory_documents"
# Optional connection tuning (defaults: 10000ms connect, 5000ms server selection, pool 20)
$env:AXON_MONGODB_CONNECT_TIMEOUT_MS="10000"
$env:AXON_MONGODB_SERVER_SELECTION_TIMEOUT_MS="5000"
$env:AXON_MONGODB_MAX_POOL_SIZE="20"
cargo run -p axon-api
```

The API listens on `http://127.0.0.1:8080`. `GET /health` is the public liveness probe and `GET /ready` is the public SQLite/MongoDB readiness probe; workspace routes require a verified Firebase ID token.

MongoDB document storage is optional for local development. Set `AXON_MONGODB_URI` to enable it. Set `AXON_ENV=production` or `AXON_MEMORY_DOCUMENTS_REQUIRED=true` to make MongoDB a required startup dependency. The MongoDB adapter stores immutable document versions; SQLite continues to own workspace identity, document metadata, lifecycle and outbox state. See [MongoDB setup](MONGODB_SETUP.md).

## 2. Start the web client

In a second terminal:

```sh
cd frontend/apps/web
npm install
npm run dev
```

Open the Vite URL printed in the terminal (normally `http://127.0.0.1:5173`). The dev server proxies API requests to the local Axum process.

## 3. Start the mobile client

```sh
cd frontend/apps/mobile
npm install
npx expo start
```

For an Android emulator, set `EXPO_PUBLIC_AXON_API_URL=http://10.0.2.2:8080` before starting Expo. For a physical Android device connected over USB, keep the API bound to loopback and run:

```sh
adb reverse tcp:8080 tcp:8080
```

Then set `EXPO_PUBLIC_AXON_API_URL=http://127.0.0.1:8080`. Do not enable remote API binding on an untrusted network.

## 4. Sign in and use the workspace UI

Open the web or mobile app and create an account or sign in with Firebase Email/Password. The API provisions one personal workspace for the Firebase UID on first successful authenticated request. The clients attach the current Firebase ID token as a bearer token automatically.

The API rejects unauthenticated workspace requests. For direct API testing, first obtain an ID token through Firebase Auth and pass it as `Authorization: Bearer $FIREBASE_ID_TOKEN`.

## 5. Manual API workflow

Create a workspace:

```sh
curl -X POST http://127.0.0.1:8080/api/v1/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name":"Funspot engineering"}'
```

Copy the returned workspace UUID into `WORKSPACE_ID`, then register two manually represented agents:

```sh
curl -X POST "http://127.0.0.1:8080/api/v1/workspaces/$WORKSPACE_ID/agents" \
  -H "Content-Type: application/json" \
  -d '{"display_name":"Claude reviewer","provider":"claude"}'

curl -X POST "http://127.0.0.1:8080/api/v1/workspaces/$WORKSPACE_ID/agents" \
  -H "Content-Type: application/json" \
  -d '{"display_name":"ChatGPT implementer","provider":"chatgpt"}'
```

Capture a piece of context as an artifact:

```sh
curl -X POST "http://127.0.0.1:8080/api/v1/workspaces/$WORKSPACE_ID/artifacts" \
  -H "Content-Type: application/json" \
  -d '{"title":"Bug report","content":"Describe the issue and relevant evidence.","source_agent_id":null}'
```

Create a task using the assigned agent UUID and any selected artifact UUIDs:

```sh
curl -X POST "http://127.0.0.1:8080/api/v1/workspaces/$WORKSPACE_ID/tasks" \
  -H "Content-Type: application/json" \
  -d '{"title":"Review the issue","objective":"Investigate the cause and return evidence-backed recommendations.","assignee_agent_id":"AGENT_UUID","context_artifact_ids":["ARTIFACT_UUID"]}'
```

The workspace UI can create and advance tasks. To simulate the manual handoff, acknowledge the task, move it to `in_progress`, paste the receiving assistant's response into the task response field, and submit it. Axon stores that response as a linked artifact. Then move the task into review, accept it or request changes, and complete it when accepted.

## Current limitations

- MongoDB document creation is queued durably in SQLite and delivered asynchronously. Direct retrieval by memory ID/version is available to a workspace-authorized registered agent after SQLite approval and MongoDB hash verification. The current retrieval provider is deterministic lexical hashing, not a neural embedding. Project-scope authorization remains fail-closed. The combined session-context endpoint resolves the session's pinned epoch, returns progressive SQLite memories, and ranks exact MongoDB document versions from that epoch through QKV attention and Qoin with provenance and a bounded context budget. Supply `query_text` when the SQLite progressive vector model differs from the MongoDB document-key provider.
- The current memory-document creation API supports workspace-shared, task-shared and agent-private scopes. Project-shared creation is deliberately rejected until a persisted project-membership authorization model exists. Agent-specific memory and session APIs are restricted to the Firebase user who registered the agent. This prevents cross-member impersonation; independent cryptographic credentials for agent processes are still not implemented.
- Agent names/provider labels do not connect to provider APIs. The person using the UI still performs manual provider handoff.
- SQLite must use a persistent volume in production. MongoDB backups and SQLite backups must both be configured.
- Never place secrets or sensitive personal data in this prototype.


## 6. Versioned memory documents

Create a proposed workspace-shared memory document. It is committed to SQLite first and delivered to MongoDB asynchronously:

```sh
curl -X POST "http://127.0.0.1:8080/api/v1/workspaces/$WORKSPACE_ID/memory/documents" \
  -H "Authorization: Bearer $FIREBASE_ID_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{
    "scope":"workspace_shared",
    "document_type":"technical_decision",
    "title":"Memory persistence decision",
    "content":"SQLite owns metadata and lifecycle; MongoDB stores immutable document versions.",
    "tags":["architecture","memory"]
  }'
```

The response includes `memory_id`, `version`, and `content_hash`. The lifecycle starts as `proposed`. Approve that exact version:

```sh
curl -X POST "http://127.0.0.1:8080/api/v1/workspaces/$WORKSPACE_ID/memory/documents/$MEMORY_ID/1/approve" \
  -H "Authorization: Bearer $FIREBASE_ID_TOKEN"
```

A registered agent in the workspace can then retrieve the exact version by ID:

```sh
curl "http://127.0.0.1:8080/api/v1/workspaces/$WORKSPACE_ID/agents/$AGENT_ID/memory-documents/$MEMORY_ID/1" \
  -H "Authorization: Bearer $FIREBASE_ID_TOKEN"
```

For task-shared memory, supply `task_id` as a query parameter and the task must be assigned to the requesting agent. Agent-private memory is only returned when its stored owner agent matches the agent ID in the route. Project-shared memory is intentionally unavailable until project membership is persisted. Dead-letter events with valid retained payloads can be requeued through the authenticated `retry-sync` endpoint; this operation is audited.


Search authorized, approved MongoDB documents through the deterministic lexical QKV fallback and Qoin memory-ranking layer:

```sh
curl -X POST "http://127.0.0.1:8080/api/v1/workspaces/$WORKSPACE_ID/agents/$AGENT_ID/memory-documents/search" \
  -H "Authorization: Bearer $FIREBASE_ID_TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"query":"immutable memory snapshots","limit":10,"tags":["architecture"]}'
```

The response identifies its retrieval mode as `deterministic_lexical_qkv_v1`. This is a stable hashed lexical representation, not a neural embedding. Results include QKV attention and Qoin score breakdowns, expose both algorithm versions, and are bounded to a 64 KiB serialized-result budget with an explicit `results_truncated` flag.


## 7. Memory operations

Run a bounded, read-only reconciliation report for the workspace:

```sh
curl "http://127.0.0.1:8080/api/v1/workspaces/$WORKSPACE_ID/memory/reconciliation?limit=1000" \\
  -H "Authorization: Bearer $FIREBASE_ID_TOKEN"
```

The report contains memory IDs, version mismatch categories, and outbox queue counts—not document contents. It does not repair either database. A workspace member can retry a dead-letter version with a retained payload:

```sh
curl -X POST "http://127.0.0.1:8080/api/v1/workspaces/$WORKSPACE_ID/memory/documents/$MEMORY_ID/$VERSION/retry-sync" \\
  -H "Authorization: Bearer $FIREBASE_ID_TOKEN"
```

The retry is recorded in SQLite audit history. This endpoint is workspace-member scoped because Axon does not yet have role-based operator permissions; restrict it before production use.
