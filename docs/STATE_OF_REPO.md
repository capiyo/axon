# State of Repository

**Last updated:** 2026-11-28

**Product identity:** Axon (working name; public trademark/domain clearance remains outstanding).

## Repository

- Repository: `capiyo/axon`
- GitHub default branch setting: `main` (must be changed to `master` in repository settings; the connected GitHub operations available here cannot change this setting).
- Authoritative development branch: `master` (all implementation, fixes, tests, documentation, configuration, and integration changes are committed directly here).
- Other branches are legacy consolidation inputs only; do not use them for new development. Port verified, non-duplicate changes to `master` without overwriting unrelated work.
- At this update, `master` is 1,046 commits ahead of `main`; `main` is stale and must not replace `master`.
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
- [x] Verified bearer-token authentication using RS256 and Google's Firebase Secure Token JWKS (Firebase is currently the token/session bridge for Google sign-in).
- [x] Project-specific issuer and audience validation, expiry validation, and a 60-second clock-skew allowance.
- [x] JWKS caching using the upstream cache lifetime, refresh on unknown key IDs for key rotation, and a short forced-refresh cooldown.
- [x] Persistent `users` and `workspace_memberships` tables; the Google-authenticated Firebase UID maps to an internal Axon user ID.
- [x] One personal workspace per Google-authenticated identity, created idempotently on first successful authentication; uniqueness is enforced by provider + subject and creation is transactional.
- [x] Workspace membership is checked at the HTTP boundary. Requests for another user's workspace or task resources return not-found rather than forbidden.
- [x] All registered API routes require a verified bearer token except `GET /health` and `GET /ready`.
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

### Supervisor orchestration and model governance

- [x] Persistent supervisor runs are stored through the SQLite entity repository and restored when the API starts.
- [x] Workspace-scoped routes cover run creation/listing, checklist submission, worker results, deterministic checks, independent reviews, and bounded repair.
- [x] The deterministic 2-2-2 supervision policy is represented in the run state machine; Quick and Normal assignments use their configured reduced stages, while Important and unassigned runs require the full independent stages.
- [x] Model assignment decisions are persisted with their supervisor run, including candidate sets, applied filters, selected model, rationale, and estimated cost.
- [x] Daily per-user model budget reservations are atomic in SQLite and emit audit events; a configured daily limit defaults to 10,000,000 micro-units.
- [x] A scheduled model catalogue worker refreshes configured active providers daily. New models remain unapproved candidates; changes to approved model pricing are flagged for review rather than silently applied.
- [x] Workspace capability-consensus polls are exposed through authenticated routes and persisted in MongoDB; memory context references must resolve to active records in the same workspace.
- [x] Authorization tests now include all supervisor and capability routes for missing/expired tokens and cross-workspace access.
- [ ] Live provider credentials, production catalogue endpoints, real inference execution, measured billing reconciliation, MongoDB integration tests, and production deployment verification remain environment-dependent and are not claimed as complete.

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

- Google Sign-In is now the only user-facing web/mobile authentication option. Web uses the Google provider popup; mobile uses Expo AuthSession to obtain a Google ID token and exchange it for a Firebase session credential. The API continues to verify Firebase ID tokens. Remaining gates: configure the Google provider and OAuth client IDs in the identity project, add authorized domains and Android signing fingerprints, install/resolve the new Expo dependencies, and run web/mobile builds plus real-device sign-in tests.
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

- The Axum process currently refuses to start unless `FIREBASE_PROJECT_ID` is configured; this reflects the Firebase token/session bridge used for Google sign-in.
- `AXON_DATABASE_URL` defaults to `sqlite://axon.db`. Production deployment must mount a persistent volume or use a separately reviewed durable database adapter; ephemeral container filesystems are not sufficient.
- `AXON_ENABLE_SIMULATOR` defaults to false. Do not enable simulator ingress in production.
- Firebase tokens are never written to logs. Request bodies and user message text are not logged by the authentication middleware.
- The backend has no password sign-in endpoint. Google sign-in is initiated by the clients through the configured Google provider; the API accepts only verified Firebase ID tokens from the identity bridge.
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


## Mandatory master-only development policy (2026-11-27)

- `master` is the only authoritative branch for all future implementation, fixes, tests, docs, refactors, dependency updates, and releases.
- Do not create or use new feature, fix, implementation, production, foundation, or integration branches. Work directly on the latest `master`.
- Legacy branches are historical sources only. Port reviewed, non-duplicate changes into `master`; never force-merge divergent histories or overwrite current code.
- Never force-push or reset `master`. A task is complete only after its code is committed on `master`, relevant tests have run, and the resulting commit/CI status is verified.
- GitHub repository settings must set `master` as the default branch and enforce branch restrictions/rulesets. Documentation and CI triggers alone do not enforce repository permissions.
- Keep Axon independent of Funspot and Qoin; do not reintroduce retired integrations.
