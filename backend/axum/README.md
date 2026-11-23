# Axon Axum API

Axon's Rust/Axum API provides workspace, agent, artifact and task orchestration backed by SQLite. All API routes require a verified Firebase ID token except GET /health.

## Run locally

Configure a Firebase project dedicated to Axon. Do not reuse Funspot's Firebase project.

    export FIREBASE_PROJECT_ID="your-axon-firebase-project-id"
    export AXON_DATABASE_URL="sqlite://axon.db"
    export AXON_HOST="127.0.0.1"
    export AXON_PORT="8080"
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

## Vector decisions and agent memory

Model assignment uses Axon's deterministic vector engine. Each eligible model's observed vector combines completion, independent supervisor pass, latency efficiency, cost efficiency and retry resilience. Task-level weights change the trade-off for Quick, Normal and Important work. Curated model order remains the cold-start prior; observed history gains influence as its sample count grows. Eligibility, privacy, provider diversity and budget checks remain hard constraints ahead of scoring.

The supervisor applies a second vector policy to the run. Risk and ambiguity currently use a conservative deterministic keyword/shape heuristic, not a semantic safety classifier; risky or materially ambiguous tasks are raised to Important supervision, and the budget fallback cannot silently downgrade that requirement. Existing authorization, privacy and budget filters remain hard constraints. The selected action (execute, delegate, split, request evidence, escalate or abstain) is recorded in the run notice. Terminal accepted/failed runs create or update category/role history using observed completion and supervisor-pass outcomes. Initial cost and latency fields come from registry estimates, and retry rate stays neutral until a real user-retry signal exists; this workflow does not fabricate billed cost or provider latency measurements.

Memory is separate from chat history and uses four scopes:
- `workspace`: shared with agents in that workspace.
- `task`: available only to the task's assigned agent.
- `agent`: private to its owner unless an explicit grant is active.
- `execution`: task-bound, explicitly granted and required to have an expiry.

New memory is always a proposal with source provenance. A workspace member reviews it with approve/reject before it becomes verified; unverified workspace/task/execution memory is not shared into agent context, and private proposals remain visible only to their owning agent. Sharing and revocation are explicit, and grants can only be created for verified memory. Retrieval checks workspace, scope, grant, expiry and sensitivity before relevance scoring. The allocator uses deterministic lexical relevance plus freshness, authority, confidence and verification, then selects by utility per token under the requested budget. The current member API is capped at Internal sensitivity; higher-sensitivity access requires a future role policy. Expired content and its versions are purged before allocation, with a content-free audit tombstone. Allocation audit rows retain selection metadata, not copied memory content.

Memory endpoints (all require Firebase authentication and workspace membership):

- POST `/api/v1/workspaces/{workspace_id}/memory` — propose memory. JSON example:
  `{"scope":"workspace","scope_id":null,"owner_agent_id":null,"content":"Use append-only event records for chat memory.","source":"design review","sensitivity":"internal","expires_at":null}`
- POST `/api/v1/workspaces/{workspace_id}/memory/{memory_id}/review` — JSON: `{"decision":"approve"}` or `{"decision":"reject"}`.
- POST `/api/v1/workspaces/{workspace_id}/memory/{memory_id}/share` — JSON: `{"agent_id":"<agent-uuid>"}`.
- DELETE `/api/v1/workspaces/{workspace_id}/memory/{memory_id}/share/{agent_id}` — revoke an agent grant.
- POST `/api/v1/workspaces/{workspace_id}/memory/allocate` — JSON: `{"task_id":null,"agent_id":"<agent-uuid>","token_budget":8192,"query":"What are the approved memory decisions for this task?"}`. The response includes selected memory content and provenance, plus token totals and excluded IDs.

For task memory, set `scope_id` to the task UUID and allocate with that task's assigned agent. For agent memory, set `owner_agent_id`. Execution memory must include `scope_id` and `expires_at`.

## Simulator routes

- GET /api/v1/sim/devices/{device_id}/telemetry
- POST /api/v1/sim/devices/{device_id}/telemetry

Simulator routes require authentication and are hidden unless AXON_ENABLE_SIMULATOR=true. Keep this flag false in production. Simulator events remain bounded and process-local; they are not durable device telemetry or physical-device ingress.

## Audit and data lifecycle

Mutations record the authenticated internal user as the audit actor. Entity writes and audit rows commit in the same transaction. Audit rows are append-only at the SQLite layer.

Account/data deletion is design-only; no deletion route or job exists. See docs/ACCOUNT_AND_DATA_DELETION_DESIGN.md before implementing destructive account workflows.
