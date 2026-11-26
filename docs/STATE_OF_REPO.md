# State of Repository

**Last updated:** 2026-11-26

**Product identity:** Axon (working name; public trademark/domain clearance remains outstanding).

## Repository

- Repository: `capiyo/axon`
- Default branch: `main`
- Active implementation branch: `production/phase-b-chats-memory`
- Repository: [github.com/capiyo/axon](https://github.com/capiyo/axon)
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

### Dual-database memory foundation (in progress)

- [x] Added a separate MongoDB memory-document repository; SQLite remains the relational and authorization authority.
- [x] Added versioned immutable MongoDB document writes with a unique `memory_id + version` index and SHA-256 content verification.
- [x] Added SQLite document-version metadata and transactional outbox migrations (`0010`, `0011`).
- [x] Added a leased outbox worker with idempotent delivery, bounded retries, dead-letter state, audited retry-sync recovery, lease-safe completion/failure transactions, and graceful shutdown.
- [x] Added authenticated memory-document creation for workspace-shared, task-shared, and agent-private scopes; agent-private creation is bound to the registering user who owns the agent.
- [x] Added validated MongoDB configuration, redacted URI debug output, bounded connection timeouts, bounded pool size, production-required startup behavior, and separate SQLite/MongoDB readiness probes.
- [x] Added approved, scope-checked agent document retrieval and query search using deterministic lexical QKV attention followed by configurable, versioned Qoin memory scoring. Search responses return the scoring policy and per-candidate breakdown.
- [x] Immutable memory-document epoch metadata now pins exact document version, content hash, source sequence, QKV version, and Qoin scoring version.
- [ ] A production neural embedding provider remains outstanding; the asynchronous provider abstraction is injected through AppState and defaults to deterministic lexical QKV keys with source-text integrity hashes.
- [x] Agent ownership is persisted in the serialized Agent entity; agent-specific document, handover, and session routes reject cross-member agent impersonation.
- [x] A session-bound endpoint resolves the session's immutable epoch and hydrates/ranks exact MongoDB document versions through QKV and Qoin.
- [x] Persistent SQLite projects and project memberships with owner/admin/member/viewer roles, workspace-owner project creation, project-role grants/revocation, and workspace-membership revocation cascading to project access.
- [x] Project-shared memory creation, search, direct retrieval, version history, approval, and immutable session hydration enforce current project membership; scope promotion is not yet implemented.
- [x] Memory archive, restore, and logical deletion create new immutable versions through the SQLite outbox; lifecycle tombstones suppress older approved versions from normal retrieval and new epochs while existing session epochs retain their exact pinned versions.
- [x] Independent hashed agent credentials with 90-day expiry, atomic rotation/revocation, current-owner checks, and middleware binding to a single agent's memory routes.
- [x] Persistent, auditable retrieval feedback bound to exact versions pinned in authenticated sessions; Qoin scoring v2 incorporates smoothed feedback signals through a fingerprinted `feedback_weight` while legacy v1 epochs retain their prior ranking behavior.
- [x] Added audited dead-letter recovery for valid retained outbox payloads.
- [x] Added bounded, workspace-scoped read-only SQLite/MongoDB reconciliation reports.
- [ ] Automatic reconciliation repair, searchable vector index rebuild, neural embeddings, scope promotion, physical deletion/retention purge, general-purpose capability grants for all resource families, legacy agent ownership backfill, and full MongoDB integration/security tests remain outstanding.

### Frontend and design
- [x] React + TypeScript + Vite web workspace.
- [x] React Native + Expo mobile workspace.
- [x] Shared frontend API contract types.
- [x] Axon-owned `frontend/packages/theme` package with named TypeScript tokens and CSS custom properties.
- [x] Web and mobile consume the Qoin-derived dark navy, slate and lime palette.

### Robotics and connector foundation
- [x] AXP robotics device-registration, telemetry, non-executable command-proposal, and safety-state schemas.
- [x] Pure Rust proposal validation for identity matching, requester authorization, emergency stop, interlocks, operating mode, expiry, sequence, idempotency-key shape, and velocity bounds.
- [x] Robotics safety boundary documented; no hardware dispatch or actuator endpoint.
- [x] Python Claude-export to DeepSeek API connector with AXP normalization, explicit approval gating, CLI preview, and unit tests.
- [x] Python connector Ruff/pytest CI job.

## Not implemented / production gates

- Web and Expo mobile Firebase email/password sign-in and account creation, session restoration, sign-out, and Firebase ID-token injection on authenticated API requests. Client configuration is supplied through app-specific public Firebase environment variables; no Firebase credentials are committed. Google, email-link and phone-first sign-in providers remain outside the current email/password implementation.
- Provider-side sign-in abuse controls and any client-specific phone/SMS quota configuration. Axon currently exposes no password/SMS sign-in endpoint; credentials should go directly through Firebase Auth, not through Axon's API.
- Invitations, workspace membership management UI, role/capability grant persistence and management APIs.
- A news consumer and provider/model-driven assistant response generation. Chat messages are persisted, but Axon does not invent assistant replies.
- Durable simulator telemetry, device cryptographic enrollment, persistent replay/idempotency enforcement, device gateway and hardware-in-the-loop tests.
- Artifact content hashing, classification enforcement, context bundle read API and retention controls.
- Account/data deletion execution, legal retention policy and deletion worker; see [ACCOUNT_AND_DATA_DELETION_DESIGN.md](ACCOUNT_AND_DATA_DELETION_DESIGN.md).
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

The historical Phase A CI run referenced in earlier repository notes belongs to the former `capiyo/Qapten-` repository and is not evidence for the current `capiyo/axon` branch. Current-branch Rust formatting, compilation, tests, Clippy, and MongoDB integration status must be taken from checks run against the current branch. They have not been executed from this editing environment.

## Architecture direction

- `frontend/` is the client layer: web, mobile and frontend-owned shared TypeScript packages.
- `backend/` is the server layer: Axum owns API behavior and authoritative domain state.
- `contracts/` is the language-neutral, versioned protocol boundary.
- SQLite is the current durable adapter for Axon workspace/agent/artifact/task entities and audit events; production persistence/backup topology must be explicitly configured.
- Axum remains the orchestration authority. Drogon and Zig remain evidence-driven extension points, not duplicated orchestration services.


## Dual-database memory implementation note

The current implementation includes SQLite-authoritative metadata, immutable versioned MongoDB documents, a recoverable transactional outbox with audited dead-letter retry, deterministic lexical QKV retrieval, configurable versioned Qoin v2 ranking with session-pinned retrieval feedback, immutable document-version epoch pins, combined session-context retrieval, persistent workspace/project memberships with project-scope authorization, bounded read-only reconciliation, and separate database readiness probes. It is not a completed production memory system: neural embeddings, searchable vector indexing/rebuild, automatic repair, scope promotion, physical deletion/retention purge, general-purpose persisted capability grants for all resource families, provider-backed agent execution, live MongoDB failure/restart tests, and performance/restore drills remain outstanding. See [Memory Architecture](MEMORY_ARCHITECTURE.md), [MongoDB Setup](MONGODB_SETUP.md), [QKV Retrieval](QKV_RETRIEVAL.md), [Qoin Memory Intelligence](QOIN_MEMORY_INTELLIGENCE.md), and [Memory Testing](MEMORY_TESTING.md). Current-branch Rust compilation and integration tests must pass before production use.
