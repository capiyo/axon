# Axon Axum API

Axon's Rust/Axum API provides workspace, agent, artifact and task orchestration backed by SQLite. All API routes require a verified Firebase ID token except GET /health.

## Run locally

Configure a Firebase project dedicated to Axon. Do not reuse Funspot's Firebase project.

    export FIREBASE_PROJECT_ID="your-axon-firebase-project-id"
    export AXON_DATABASE_URL="sqlite://axon.db"
    export AXON_HOST="127.0.0.1"
    export AXON_PORT="8080"
    # Optional: comma-separated Firebase UIDs allowed to administer provider/model registry.
    export AXON_PLATFORM_ADMIN_UIDS="firebase-uid-1,firebase-uid-2"
    # Optional: comma-separated browser origins for a separately hosted web client.
    export AXON_ALLOWED_ORIGINS="https://axon.example.com"
    # Optional: private service URL for the Qoin Axum research API.
    export QOIN_API_URL="http://127.0.0.1:8081"
    # Simulator is disabled unless explicitly enabled for development:
    export AXON_ENABLE_SIMULATOR="false"

    cargo run -p axon-api

The server refuses to start without FIREBASE_PROJECT_ID. The default database file is axon.db; production must use a persistent mounted volume or a separately reviewed database adapter with backups and restore testing.
For hosted platforms that inject `PORT` (including Render), Axon uses that port when `AXON_PORT` is unset and defaults to `0.0.0.0` when `AXON_HOST` is unset. An explicit `AXON_PORT` or `AXON_HOST` takes precedence. Local runs without `PORT` retain the loopback default `127.0.0.1:8080`.


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
- POST /api/v1/tasks/{task_id}/execute
- POST /api/v1/tasks/{task_id}/submit
- POST /api/v1/tasks/{task_id}/transition

## Chats and Explore

- GET /api/v1/workspaces/{workspace_id}/chats
- POST /api/v1/workspaces/{workspace_id}/chats
- GET /api/v1/workspaces/{workspace_id}/chats/{chat_id}
- POST /api/v1/workspaces/{workspace_id}/chats/{chat_id}/messages
- PATCH /api/v1/workspaces/{workspace_id}/chats/{chat_id}/archive
- POST /api/v1/workspaces/{workspace_id}/chats/{chat_id}/read
- POST /api/v1/workspaces/{workspace_id}/chats/{chat_id}/like
- GET /api/v1/explore/agents
- GET /api/v1/explore/agents/{listing_id}
- POST /api/v1/explore/agents/{listing_id}/comments

Chat listing responses include per-user unread count and reaction state, message count, and the latest message. Chat and memory routes enforce workspace membership before accessing private records.

## Provider-backed task execution

The task execution route uses the assigned agent's provider name to load credentials from environment variables. Provider slugs are normalized to uppercase ASCII letters and digits. For an agent whose provider is openai, configure AXON_AGENT_OPENAI_BASE_URL, AXON_AGENT_OPENAI_API_KEY, and AXON_AGENT_OPENAI_MODEL. For an agent whose provider is anthropic or claude, use the corresponding AXON_AGENT_ANTHROPIC_* or AXON_AGENT_CLAUDE_* variables. Base URLs are service roots; Axon appends the provider's chat-completions or messages path. Keep API keys in the deployment secret manager, never in agent records or source control. A successful execution stores the response as a workspace artifact and moves the task to submitted. A provider failure leaves the task in progress so it can be retried.

## Platform model registry administration

Set `AXON_PLATFORM_ADMIN_UIDS` to an explicit comma-separated allowlist of Firebase UIDs. The backend fails closed: users not on this list cannot access provider/model registry administration, even when authenticated. Do not put email addresses or secrets in this variable. Registry routes are under `/api/v1/admin/` and include provider management, provider data-terms review, model candidate review, evaluations, stats, flags and assignment history.

## Qoin market-vector integration

Axon can consume the current attended market-state vector from Qoin's authenticated-network/internal API. Configure the Axon backend with:

    QOIN_API_URL=http://127.0.0.1:8081

In a container deployment, use the Qoin service name and internal port, not localhost (for example, `http://qoin-api:8081`). Qoin must expose `GET /internal/vector?symbol=BTCUSDT` to the Axon backend over a private service network. In container deployments, configure Qoin to bind its internal listener to `0.0.0.0:8081` and set Axon's URL to the private Qoin service name (for example `http://qoin-api:8081`). Do not expose that internal route publicly.

The Axon bridge requires six finite `attended_features` values and a non-empty `strategy_version`. Agent handovers use the latest vector. When an approved memory is indexed, Axon reads the original source chat-message timestamp when the memory references a message; otherwise it uses the memory's immutable `created_at`. It requests Qoin's `/internal/vector/history` endpoint at that event time. It never substitutes the current vector for an old memory. The model version is pinned to each indexed memory; QKV refuses to compare different Qoin strategy versions. Newly indexed entries start with neutral outcome/evidence scores (0.5) until an outcome/evidence evaluation pipeline supplies measured values. Project-shared indexing is not exposed through this bridge until Axon has a validated project-membership resource. If the strategy version changes, historical memories from the old version cannot be compared with current vectors. They require an explicit historical re-projection/rebuild into the new Qoin vector space; simply re-indexing the old stored vector is insufficient.

Web and mobile users can load a Qoin symbol vector, index individual original chat messages and approved project memories against their original event-time market state, initialize an agent's first context using the 20% budget / measured 80% contribution target, and progressively retrieve deferred memories. For a separately hosted web client, set `VITE_AXON_API_URL` at build time and allow its exact origin in `AXON_ALLOWED_ORIGINS`; the API uses bearer-token authorization, not cookies. Memory indexing only adds to the live index; the hourly publisher remains the only path that creates a new immutable epoch. Newly indexed memories become available to handovers after the next hourly epoch publication.

This integration maps historical decisions to similar Qoin market states. Qoin's six-dimensional market vector is **not** a semantic text embedding. It does not infer meaning from arbitrary chat prose; use a separate versioned text-embedding provider if semantic chat retrieval is required.

## Qoin memory routes

- GET `/api/v1/workspaces/{workspace_id}/memory/qoin-vector?symbol=BTCUSDT`
- POST `/api/v1/workspaces/{workspace_id}/chats/{chat_id}/memory-index/{memory_id}/qoin`
- POST `/api/v1/workspaces/{workspace_id}/chats/{chat_id}/messages/{message_id}/qoin-index`
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

The optional `axon-memory` and `axon-route` Python connector commands use the same Firebase-authenticated API boundary as the web and mobile clients. Conversation imports, memory proposals/reviews, active-memory context reads, and capability-poll operations are restricted to workspace members. MongoDB must be configured for the legacy conversation-import and capability-poll repository; without it these endpoints return service unavailable rather than silently falling back to in-memory storage.

The MongoDB-backed memory import API is versioned and workspace-scoped: `POST /api/v1/workspaces/:workspace_id/memory/imports` stores an imported conversation, `GET /memory/imports/:import_id` retrieves it, `POST /memories` creates proposed candidates tied to that import, `GET /memories` lists records with optional status filtering, `GET /memory/context` returns active records only, and `POST /memories/:memory_id/review` performs version-checked approve/reject/supersede transitions. All of these routes require a verified Firebase identity and workspace membership.

Migration `0021_unique_personal_workspace_id.sql` preserves the original one-user-per-personal-workspace invariant after the personal workspace column was added to the normalized Firebase identity schema.

Conversation import retrieval and memory listing preserve workspace scoping: import lookup never crosses workspaces, and memory listing supports an optional lifecycle-status filter while the context endpoint is restricted to active records.
