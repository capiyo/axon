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
