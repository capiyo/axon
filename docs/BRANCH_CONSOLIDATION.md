# Axon branch consolidation record

**Authoritative branch:** `master`  
**Repository:** `capiyo/axon`  
**Policy:** All future implementation and integration commits go directly to `master`. Do not create feature or development branches.

## Consolidation verification

The historical branches below were compared against `master`. Their feature areas and changed paths were checked against the current master tree. The branches are divergent historical snapshots, so their complete tips must not be merged wholesale: that would reintroduce old implementations over newer master work.

| Historical branch | Work area | Master disposition |
|---|---|---|
| `implementation/axon-social-workspace` | Agent runtime, Explore/community, task execution, Python persistence, workspace UI | Ported/evolved on master. Agent runtime, Explore, task execution, persistence connector, and corresponding web/mobile surfaces are present. |
| `feature/funspot-ui-parity` | Web/mobile workspace presentation and theme | Superseded by the current master screen architecture and theme. |
| `refactor/axon-funspot-screen-architecture` | Hub/Chats/Explore/Sessions screens, cards, modals, architecture | Ported/evolved on master. Current screen, card, modal, and architecture files are present. |
| `feature/supervisor-engine-bridge` | Provider adapter, model registry, supervisor, model discovery, vector decisions, tests | Ported/evolved on master. Provider, registry, supervisor, routing, and decision modules are present. |
| `feature/axon-memory-consensus-routing` | Capability consensus, memory repository, provider routing, contracts | Ported/evolved on master. Capability, memory, routing modules, connector code, schemas, and docs are present. |
| `feature/memory-vector-reconstruction-v1` | Historical context, immutable memory epochs, progressive retrieval, QKV | Ported/evolved on master. Memory architecture/intelligence, migrations, and retrieval code are present. |
| `feature/qoin-memory-fullstack-integration` | Qoin vector scoring and full-stack memory integration | Ported/evolved on master. Qoin vector and memory integration code are present. |
| `foundation/axon-desktop-shell` | Tauri 2 desktop shell and persistence-boundary ADR | Ported on master. Desktop Tauri configuration, Rust host, assets, and docs are present. |
| `foundation/axum-web-skeleton` | Initial Axum/Firebase foundation and account deletion design | Evolved on master. Current Firebase auth/middleware and storage architecture supersede the old monolithic `auth.rs`; account deletion remains explicitly design-only. |
| `production/phase-a-firebase-auth` | Firebase authentication | Behind master; its work is represented by the current auth implementation. |
| `production/phase-b-chats-memory` | Chats and persistent memory | Behind master; its work is represented by the current chat and memory implementation. |
| `production/phase-b-validation` | Chat/memory validation and frontend wiring | Evolved on master; the relevant code and tests are present. |
| `main` | Old default branch | Historical only; master contains the active implementation. |

## Migration reconciliation

The historical task-lease migration was named `0010_task_execution_leases.sql`. Master already contains the same migration content as `0016_task_execution_leases.sql` (same Git blob), after newer migrations had occupied the earlier sequence. Do not add the old filename or duplicate the migration.

## Safety and verification notes

- No divergent historical branch tip was force-merged into master.
- Existing master implementations were retained where they supersede earlier branch snapshots.
- A path being present is not, by itself, proof that every historical implementation detail is identical; behavior must be validated by the repository CI and targeted tests.
- This record does not claim production readiness. Provider credentials/inference, billing reconciliation, MongoDB integration testing, deployment configuration, and other gates remain governed by `docs/STATE_OF_REPO.md`.
- Historical branches are retained as read-only recovery sources. Do not delete them as part of consolidation unless explicitly requested.

## Required repository setting

GitHub's default branch setting must be changed from `main` to `master` in repository settings. The connected GitHub operations used for this consolidation do not expose that administrative setting, so it remains a manual repository-owner action. Until changed, all write operations must explicitly target `master`.

## Operating rule

Every new Axon change goes directly to `master`. Do not open feature branches or leave completed implementation work stranded outside `master`.


## File-level reconciliation of historical branch-only paths

A recursive Git tree comparison was performed for all 13 non-master branches against the current master tree. The only paths present on historical branch trees but absent by exact path on master were:

| Historical path | Disposition on master |
|---|---|
| `backend/axum/migrations/0004_model_registry.sql` | Exact same Git blob is already present as `0018_model_registry.sql`. |
| `backend/axum/migrations/0005_agent_memory.sql` | Exact same Git blob is already present as `0019_agent_memory.sql`. |
| `backend/axum/migrations/0010_task_execution_leases.sql` | Exact same Git blob is already present as `0016_task_execution_leases.sql`. |
| `backend/axum/migrations/0012_immutable_memory_document_epoch_items.sql` | Exact same Git blob is already present as `0017_immutable_memory_document_epoch_items.sql`. |
| `backend/axum/migrations/0002_firebase_users_memberships.sql` | Superseded by the current `0002_users_and_memberships.sql` plus subsequent membership-integrity migrations; the old schema is not reintroduced. |
| `backend/axum/src/auth.rs` | Superseded by the separated `firebase_auth.rs` and `auth_middleware.rs` implementation. |
| `contracts/qep/v1/envelope.schema.json` | Renamed and updated as `contracts/axp/v1/envelope.schema.json`; the current schema identifies Axon and uses the current protocol namespace. |

The remaining branch-tree paths already exist on master. Their blob hashes differ in many cases because master contains later revisions; historical versions were not copied over newer code. This is the safe content-level consolidation result: no unique executable source file remains absent from the master tree, and migration history is not duplicated.


## Recursive branch-diff audit (2026-11-28)

A second audit used GitHub three-dot commit comparisons (master...branch) for every historical branch. This is a merge-base diff, not a filename-only comparison. Divergent branch heads still contain commits not reachable from master; aggregate merge-base diffs must not be mistaken for missing features because many files are older snapshots of code that master has since evolved.

| Branch | Branch-only commits reported | Files in merge-base diff | Changed lines |
|---|---:|---:|---:|
| feature/axon-memory-consensus-routing | 181 | 34 | 3,782 |
| feature/funspot-ui-parity | 17 | 5 | 620 |
| feature/memory-vector-reconstruction-v1 | 73 | 19 | 2,270 |
| feature/qoin-memory-fullstack-integration | 63 | 12 | 1,081 |
| feature/supervisor-engine-bridge | 278 | 47 | 9,228 |
| foundation/axon-desktop-shell | 49 | 19 | 405 |
| foundation/axum-web-skeleton | 59 | 15 | 1,234 |
| implementation/axon-social-workspace | 196 | 28 | 5,006 |
| production/phase-b-validation | 1 | 13 | 1,722 |
| refactor/axon-funspot-screen-architecture | 255 | 49 | 10,014 |
| main | 0 | 0 | 0 |
| production/phase-a-firebase-auth | 0 | 0 | 0 |
| production/phase-b-chats-memory | 0 | 0 | 0 |

These are comparison metadata, not a statement that every line is absent from master. Targeted blob checks confirmed exact master/branch matches for capability routing (capability.rs, memory.rs), Qoin vectors (qoin_vectors.rs), agent runtime (agent_runtime.rs), and the desktop Tauri host (src-tauri/src/main.rs). Targeted checks also found evolved differences in memory architecture/intelligence, supervisor/model registry, and Explore. In those cases master contains newer additions, including Qoin scoring-version validation, epoch publication/lifecycle safeguards, and Explore agent-detail retrieval, while the historical branch has older implementations. Those older files must not overwrite master.

**Correction:** the earlier tree-path reconciliation was not a recursive commit/content consolidation. It only established path coverage and migration/auth path disposition. It did not cherry-pick every historical commit or prove complete behavioral equivalence. This three-dot audit records actual divergence; code-level reconciliation and CI validation must decide each differing behavior. Do not describe every branch commit as merged merely because its file paths exist.


## Additional recursive integrations applied after the diff audit

The divergent supervisor-engine and production-validation code was reviewed at behavior level, not just by file name.

- Restored the branch's ChatGPT-export path into the current Python connector CLI. The existing provider-neutral ChatGPT parser is now reachable via `--provider chatgpt` and `--conversation-id`; explicit authenticated artifact import and response persistence are exposed through `--import-to-axon` and `--persist-response`. Added CLI regression coverage for selecting a ChatGPT conversation. The current master DeepSeek transfer and bounded context behavior were retained.
- Restored the missing `execute command` high-risk phrase in supervisor task risk classification, with a regression assertion. This keeps command-execution work from being classified as ordinary low-risk work solely because it omitted terms such as shell or production.
- Made resume briefs consistently emit empty-state values for Decisions, Open tasks, and Next / constraints, preserving the current master implementation's workspace-aware cache transaction and approved-memory filtering. Added regression assertions for the empty-state headings.

These are selective ports of branch behavior into the current master architecture; no historical branch snapshot was copied over the newer implementation.

- Ported the hosted-platform bind behavior from the social-workspace branch into the current `main.rs` without losing master’s worker lifecycle and graceful shutdown: `PORT` is honored unless `AXON_PORT` is explicitly set, and hosted runs bind `0.0.0.0` unless `AXON_HOST` is explicitly set. Documented the local-versus-hosted defaults in `backend/axum/README.md`.

- Updated `connectors/python/README.md` to replace the obsolete Claude-only/local-only description with the implemented ChatGPT selection, authenticated Axon artifact import, optional DeepSeek response persistence, and the actual token/API security boundaries.


## Further behavior-level reconciliation

The social-workspace task execution branch contained one substantive capability that was not present in master despite the frontend already sending it: a task could reference its originating chat, but the backend Task/CreateTask model and executor did not preserve or consume that reference. This has now been ported into the current master model and task flow:

- Added backward-compatible `created_by` and optional `source_chat_id` task metadata. Existing serialized task records remain deserializable.
- Task creation validates that a supplied source chat belongs to the target workspace.
- Explicit task execution now loads the source chat for the authenticated user and adds bounded approved memory, resume brief, and recent messages as clearly labelled reference data. The existing master execution lease, workspace authorization, task transition checks, QKV/Qoin retrieval, artifact isolation, provider limits, and audit persistence remain intact.
- Added serde regression tests for legacy task records and optional source-chat input, and documented the linked-task context behavior.
- Added a SQLite regression test proving a failed memory audit insert rolls back its append-only memory event in the same transaction.

The earlier feature/memory-vector, capability-consensus, Qoin-vector, agent-runtime, desktop-shell, model-registry-route, and supervisor-run test files were checked against master. Several are byte-for-byte identical; the divergent memory, supervisor, registry, and vector implementations on master are newer and were retained rather than replaced with historical versions. The old Firebase-auth skeleton and older UI snapshots were likewise superseded by the current middleware and screen architecture.

Migration reconciliation is by schema identity, not filename: the historical task-execution lease migration is present as master migration `0016_task_execution_leases.sql`; the model registry and agent memory migrations are present as `0018_model_registry.sql` and `0019_agent_memory.sql`. Do not reapply their historical lower-numbered filenames.


- Restored the historical `axon-memory` and `axon-route` connector workflows against the current authenticated API. Added workspace-member-protected conversation import, proposed-memory creation/review, and active-memory context routes; capability-poll read/write handlers now also verify workspace membership.
- Added Firebase bearer-token support to the Python connector's generic API client and wired both memory extraction and capability-majority routing through it. The previous connector calls targeted endpoints that were no longer registered and would have failed against the authenticated master API; those calls now have matching, protected server routes.
- Added API-client tests for bearer authentication/query parameters and cross-user route tests for conversation imports, memory writes/reviews, and capability-poll reads.

- Finalized the legacy MongoDB-backed memory connector integration with the current API: authenticated conversation imports, proposed memory creation, active-only context retrieval, and versioned memory review now have registered endpoints. `axon-memory` persists through those endpoints only after both provider-transfer and persistence approval; `axon-route` uses the same Firebase bearer client for capability polls and only includes active memory when explicitly requested.

- Hardened the Python capability router's workspace identifier validation before API calls; the connector regression test now exercises the Firebase-token requirement with a valid workspace UUID.

- Completed the memory connector's full route surface, including retrieving the original conversation import and status-filtered memory listing. Reused the repository's canonical import/memory validators rather than duplicating looser route-level validation.
- Restored the historical uniqueness invariant for `users.personal_workspace_id` with master migration `0021_unique_personal_workspace_id.sql` and a storage regression test. The current schema's later-added nullable column had not inherited the old branch migration's UNIQUE constraint.
- Restored TypeScript contracts for imported conversations and MongoDB-backed memory records/review requests so the authenticated API surface is represented in the shared frontend contract package.
