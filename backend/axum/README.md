# Axon Axum API

Axon's Rust/Axum API provides workspace, agent, artifact and task orchestration backed by SQLite. All API routes require a verified Firebase ID token except GET /health.

## Run locally

Configure a Firebase project dedicated to Axon. Do not reuse Funspot's Firebase project.

    export FIREBASE_PROJECT_ID="your-axon-firebase-project-id"
    export AXON_DATABASE_URL="sqlite://axon.db"
    export AXON_HOST="127.0.0.1"
    export AXON_PORT="8080"
    # Optional: comma-separated browser origins for a separately hosted web client.
    export AXON_ALLOWED_ORIGINS="https://axon.example.com"
    # Optional: private service URL for the Qoin Axum research API.
    export QOIN_API_URL="http://127.0.0.1:8081"
    # Simulator is disabled unless explicitly enabled for development:
    export AXON_ENABLE_SIMULATOR="false"

    cargo run -p axon-api

The server refuses to start without FIREBASE_PROJECT_ID. The default database file is axon.db; production must use a persistent mounted volume or a separately reviewed database adapter with backups and restore testing.

## Authentication

Send the Firebase ID token in the standard bearer header:

    Authorization: Bearer <firebase-id-token>

The verifier checks:
- RS256 signature using Google's Firebase Secure Token JWKS.
- iss = https://securetoken.google.com/<FIREBASE_PROJECT_ID>.
- aud = FIREBASE_PROJECT_ID.
- Expiry, with a 60-second clock-skew allowance.
- A non-empty Firebase subject/UID.

JWKS keys are cached according to upstream cache headers. An unknown key ID triggers a bounded refresh to support key rotation. Tokens are not logged. Authentication and workspace authorization are enforced before handlers execute. Requests for another user's workspace or task resources return 404.

The first successful authentication creates or resolves one internal Axon user and one personal workspace for the Firebase UID. Provider + subject is unique; user, workspace, membership and initial audit event are created transactionally. Workspace membership records are separate from provider identity so invitations can be added later.

Axon does not expose a password or SMS sign-in endpoint. Web/mobile clients must sign in directly through Firebase Auth and send the resulting ID token to this API. For Kenya, phone sign-in is the preferred client flow; configure Firebase SMS quotas, abuse protections and client-side resend cooldowns. Google and email-link sign-in are intended fallbacks.

## Public route

- GET /health

All other registered routes require a valid Firebase bearer token.

## API endpoints

- GET /api/v1/workspaces
- POST /api/v1/workspaces — JSON: {"name":"My workspace"}
- GET /api/v1/workspaces/{workspace_id}/audit
- GET /api/v1/workspaces/{workspace_id}/agents
- POST /api/v1/workspaces/{workspace_id}/agents — JSON: {"display_name":"Reviewer","provider":"manual"}
- GET /api/v1/workspaces/{workspace_id}/artifacts
- POST /api/v1/workspaces/{workspace_id}/artifacts — JSON: {"title":"Investigation","content":"Captured response","source_agent_id":null}
- GET /api/v1/workspaces/{workspace_id}/tasks
- POST /api/v1/workspaces/{workspace_id}/tasks — JSON: {"title":"Review the fixture status bug","objective":"Inspect the event timing logic and report evidence.","assignee_agent_id":"<agent-uuid>","context_artifact_ids":[]}
- POST /api/v1/tasks/{task_id}/submit
- POST /api/v1/tasks/{task_id}/transitions

## Qoin market-vector integration

Axon can consume the current attended market-state vector from Qoin's authenticated-network/internal API. Configure the Axon backend with:

    QOIN_API_URL=http://127.0.0.1:8081

In a container deployment, use the Qoin service name and internal port, not localhost (for example, `http://qoin-api:8081`). Qoin must expose `GET /internal/vector?symbol=BTCUSDT` to the Axon backend over a private service network. In container deployments, configure Qoin to bind its internal listener to `0.0.0.0:8081` and set Axon's URL to the private Qoin service name (for example `http://qoin-api:8081`). Do not expose that internal route publicly.

The Axon bridge requires six finite `attended_features` values and a non-empty `strategy_version`. Agent handovers use the latest vector. When an approved memory is indexed, Axon reads the original source chat-message timestamp when the memory references a message; otherwise it uses the memory's immutable `created_at`. It requests Qoin's `/internal/vector/history` endpoint at that event time. It never substitutes the current vector for an old memory. The model version is pinned to each indexed memory; QKV refuses to compare different Qoin strategy versions. Newly indexed entries start with neutral outcome/evidence scores (0.5) until an outcome/evidence evaluation pipeline supplies measured values. Project-shared indexing is not exposed through this bridge until Axon has a validated project-membership resource. If the strategy version changes, historical memories from the old version cannot be compared with current vectors. They require an explicit historical re-projection/rebuild into the new Qoin vector space; simply re-indexing the old stored vector is insufficient.

Web and mobile users can load a Qoin symbol vector, index approved chat memories with that market state, initialize an agent's first context using the 20% budget / measured 80% contribution target, and progressively retrieve deferred memories. For a separately hosted web client, set `VITE_AXON_API_URL` at build time and allow its exact origin in `AXON_ALLOWED_ORIGINS`; the API uses bearer-token authorization, not cookies. Memory indexing only adds to the live index; the hourly publisher remains the only path that creates a new immutable epoch. Newly indexed memories become available to handovers after the next hourly epoch publication.

This integration maps historical decisions to similar Qoin market states. Qoin's six-dimensional market vector is **not** a semantic text embedding. It does not infer meaning from arbitrary chat prose; use a separate versioned text-embedding provider if semantic chat retrieval is required.

## Qoin memory routes

- GET `/api/v1/workspaces/{workspace_id}/memory/qoin-vector?symbol=BTCUSDT`
- POST `/api/v1/workspaces/{workspace_id}/chats/{chat_id}/memory-index/{memory_id}/qoin`
- GET `/api/v1/workspaces/{workspace_id}/memory/epoch`
- POST `/api/v1/workspaces/{workspace_id}/agents/{agent_id}/memory-context`
- POST `/api/v1/workspaces/{workspace_id}/agents/{agent_id}/memory-sessions/{session_id}/retrieve`

## Simulator routes

- GET /api/v1/sim/devices/{device_id}/telemetry
- POST /api/v1/sim/devices/{device_id}/telemetry

Simulator routes require authentication and are hidden unless AXON_ENABLE_SIMULATOR=true. Keep this flag false in production. Simulator events remain bounded and process-local; they are not durable device telemetry or physical-device ingress.

## Audit and data lifecycle

Mutations record the authenticated internal user as the audit actor. Entity writes and audit rows commit in the same transaction. Audit rows are append-only at the SQLite layer.

Account/data deletion is design-only; no deletion route or job exists. See docs/ACCOUNT_AND_DATA_DELETION_DESIGN.md before implementing destructive account workflows.
