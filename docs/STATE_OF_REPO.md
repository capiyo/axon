# State of Repository

**Last updated:** 2026-11-22

**Product identity:** Axon (working name; public trademark/domain clearance remains outstanding).

## Repository

- Repository: `capiyo/axon` (mirrored from `capiyo/Qapten-`; source repository remains untouched).
- Default branch: `main` (historical architecture-only foundation; it does not contain the current application implementation).
- Current application baseline: `production/phase-b-chats-memory`.
- Current implementation branch: `feature/supervisor-engine-bridge`.
- Phase A and Phase B source branches were mirrored with their commit histories. Pull requests and Actions history from the source repository were not migrated; new reviews and CI must run in `capiyo/axon`.
- License: MIT (see [LICENSE](../LICENSE))

## Implemented

### Backend and persistence
- [x] Rust workspace and Axum API application.
- [x] SQLite storage adapter with versioned SQL migrations.
- [x] Existing workspace, agent, artifact and task models retained.
- [x] Workspace, agent, artifact, task creation/listing, version-checked task lifecycle transitions, and response-artifact capture.
- [x] Primary entity writes and their audit event commit atomically through the storage mutation API.
- [x] Append-only SQLite audit table with database triggers rejecting update/delete.
- [x] Audit events record the authenticated internal principal as actor for workspace, agent, artifact and task mutations.
- [x] Development simulator telemetry remains bounded and in-memory; it is not treated as durable physical-device telemetry.

### Phase A authentication and authorization
- [x] Firebase ID-token verification using RS256 and Google's Firebase Secure Token JWKS.
- [x] Project-specific issuer and audience validation, expiry validation, and a 60-second clock-skew allowance.
- [x] JWKS caching using the upstream cache lifetime, refresh on unknown key IDs for key rotation, and a short forced-refresh cooldown.
- [x] Persistent `users` and `workspace_memberships` tables; Firebase UID maps to an internal Axon user ID.
- [x] One personal workspace per Firebase UID, created idempotently on first successful authentication; uniqueness is enforced by provider + subject and creation is transactional.
- [x] Workspace membership is checked at the HTTP boundary. Requests for another user's workspace or task resources return not-found rather than forbidden.
- [x] All registered API routes require a verified bearer token except `GET /health`.
- [x] Simulator routes require authentication and are hidden unless `AXON_ENABLE_SIMULATOR=true`.
- [x] Offline tests use locally signed RSA test keys; no external network is required for token-verification tests.
- [x] Tests cover missing/expired tokens across registered API routes, wrong signing keys, JWKS key rotation, concurrent first-login provisioning, cross-user not-found behavior, and the public health route.
- [x] Account and data deletion lifecycle documented as design only; no destructive endpoint or job is enabled.

### Phase B chats and project memory
- [x] Workspace-scoped chats with newest-activity ordering and archive state.
- [x] Persistent user, assistant and system messages with optional assistant attribution.
- [x] Append-only memory event log with created, approved, edited, rejected and deleted events; current state is reconstructed by replay.
- [x] Proposed/approved/rejected memory lifecycle; only approved entries contribute to resume briefs.
- [x] Deterministic resume briefs capped at 190 words, including where-left-off, decisions, open tasks and next/constraints.
- [x] Lazy resume-brief cache keyed by chat, memory version, last message ID and prompt version; mutation transactions invalidate the cache.
- [x] Atomic SQLite transactions for chat/message/memory mutations and corresponding audit events. Audit metadata excludes message and memory text.
- [x] Authenticated workspace-scoped API routes and web/mobile chat and memory interfaces.
- [x] Backend tests cover event replay, approval/edit invalidation, workspace isolation, archive behavior and append-only enforcement.
- [x] See [CHATS_AND_PROJECT_MEMORY.md](CHATS_AND_PROJECT_MEMORY.md) for API and data contracts.

### Frontend and design
- [x] React + TypeScript + Vite web workspace.
- [x] React Native + Expo mobile workspace.
- [x] Shared frontend API contract types.
- [x] Axon-owned `frontend/packages/theme` package with named TypeScript tokens and CSS custom properties.
- [x] Web and mobile consume the Qoin-derived dark navy, slate and lime palette.
- [x] Web Firebase email/password sign-in and account creation, auth-state restoration, sign-out, and Firebase ID-token bearer injection on API requests.
- [x] Expo mobile Firebase email/password sign-in and account creation, AsyncStorage-backed auth persistence, auth-state restoration, sign-out, and Firebase ID-token bearer injection on API requests.

### Robotics and connector foundation
- [x] AXP robotics device-registration, telemetry, non-executable command-proposal, and safety-state schemas.
- [x] Pure Rust proposal validation for identity matching, requester authorization, emergency stop, interlocks, operating mode, expiry, sequence, idempotency-key shape, and velocity bounds.
- [x] Robotics safety boundary documented; no hardware dispatch or actuator endpoint.
- [x] Python Claude-export to DeepSeek API connector with AXP normalization, explicit approval gating, CLI preview, and unit tests.
- [x] Authenticated Python-to-Axon artifact API bridge for explicitly selected source conversations and explicitly approved provider responses; server derives the actor from the Firebase ID token.
- [x] Deterministic supervisor core: order-independent two-writer checklist merge, two evidence-bearing supervisor reviews, deterministic-check gating, and a hard two-repair limit.
- [x] Python connector Ruff/pytest CI job.

## Not implemented / production gates

- Google, email-link and phone-first sign-in providers remain outside the current email/password implementation. Client configuration is supplied through app-specific public Firebase environment variables; no Firebase credentials are committed.
- Provider-side sign-in abuse controls and any client-specific phone/SMS quota configuration. Axon currently exposes no password/SMS sign-in endpoint; credentials should go directly through Firebase Auth, not through Axon's API.
- Invitations, workspace membership management UI, role/capability grant persistence and management APIs.
- A news consumer and provider/model-driven assistant response generation. Chat messages are persisted, but Axon does not invent assistant replies.
- Durable simulator telemetry, device cryptographic enrollment, persistent replay/idempotency enforcement, device gateway and hardware-in-the-loop tests.
- Artifact content hashing, classification enforcement, context bundle read API and retention controls. The current API bridge writes artifacts through the existing workspace-scoped endpoint; it does not yet implement a dedicated AXP context-bundle read/write protocol.
- Account/data deletion execution, legal retention policy and deletion worker; see [ACCOUNT_AND_DATA_DELETION_DESIGN.md](ACCOUNT_AND_DATA_DELETION_DESIGN.md).
- End-to-end autonomous supervisor execution: provider API calls for both checklist writers/workers/supervisors, proof of independent agent identities, bounded context dispatch, automated deterministic CI/build/test runner integration, repair-task generation, and web/mobile run dashboard. The state machine and authenticated persistence API are implemented, but automatic provider execution and frontend workflow UI are not yet wired.
- Additional agent provider APIs, webhooks and autonomous agent execution.
- C++/Drogon service and Zig utilities (intentionally deferred until justified).
- Production deployment, secrets management, persistent-volume configuration, backups, monitoring and operational runbooks.
- Published standalone npm package for shared frontend contracts/theme.

## Runtime and security notes

- The Axum process refuses to start unless `FIREBASE_PROJECT_ID` is configured.
- `AXON_DATABASE_URL` defaults to `sqlite://axon.db`. Production deployment must mount a persistent volume or use a separately reviewed durable database adapter; ephemeral container filesystems are not sufficient.
- `AXON_ENABLE_SIMULATOR` defaults to false. Do not enable simulator ingress in production.
- Firebase tokens are never written to logs. Request bodies and user message text are not logged by the authentication middleware.
- The backend has no sign-in endpoint. Firebase Auth handles sign-in directly; the API accepts only verified Firebase ID tokens.
- Audit records are append-only. Simulator telemetry is still process-local and bounded to 500 events per device, so its deduplication window is not durable replay protection.
- Account deletion is a design only. Do not implement a hard-delete endpoint until workspace ownership, retention, audit pseudonymization and legal-hold rules are approved.
- The Firebase client application must use a separate Axon Firebase project, not Funspot's Firebase project.

## Verification

Historical source-repository GitHub Actions run [44988959132](https://github.com/capiyo/Qapten-/actions/runs/44988959132) passed all four jobs on Phase A source commit `09be25463e3f787d186105d59e65de8cd242718d`. That run is evidence for the source branch, not a new verification of this Axon mirror. The current Axon implementation branch requires its own CI run. The historical run covered:
- Rust formatting, Clippy with warnings denied, and Rust tests.
- React web production build.
- React Native TypeScript validation.
- Python connector Ruff and pytest.

The Rust suite includes offline locally signed Firebase tokens, wrong-key rejection, local JWKS rotation, per-route missing/expired-token checks, cross-user not-found checks, simulator gating and concurrent first-login provisioning. No physical Android/iOS build, hardware integration, production deployment, live Firebase project validation or external-provider sign-in has been performed.

## Architecture direction

- `frontend/` is the client layer: web, mobile and frontend-owned shared TypeScript packages.
- `backend/` is the server layer: Axum owns API behavior and authoritative domain state.
- `contracts/` is the language-neutral, versioned protocol boundary.
- SQLite is the current durable adapter for Axon workspace/agent/artifact/task entities and audit events; production persistence/backup topology must be explicitly configured.
- Axum remains the orchestration authority. Drogon and Zig remain evidence-driven extension points, not duplicated orchestration services.
- Supervisor policy, checklist merge, deterministic check evaluation and repair limits remain in Rust/Axum; provider adapters only gather proposals and evidence.
