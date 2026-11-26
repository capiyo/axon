# Memory Security

- All memory-document creation routes pass through Firebase authentication and workspace membership checks.
- Task-shared documents require a task in the same workspace.
- Agent-private documents require a registered agent in the same workspace, and the authenticated Firebase user must be the user who registered that agent.
- Project-shared document creation is rejected until project membership is persisted and enforceable.
- SQLite owns authorization metadata; MongoDB does not grant access.
- Agents must not receive MongoDB credentials or direct database access.
- Retrieved memory is untrusted content and must never override system or developer instructions.
- MongoDB connection strings and Firebase tokens must not be logged.
- Document content is not logged by the outbox worker.
- Document payloads are bounded by the API request limit and an explicit 256 KiB content limit.

Direct retrieval by memory ID/version checks SQLite workspace, scope, approval, and synchronization metadata before fetching the MongoDB version and verifying its content hash. Project-shared retrieval remains disabled. Agent-specific memory, handover, and session routes are now bound to `Agent.created_by_user_id`, preventing another workspace member from selecting an agent they did not register. Legacy agent records without an owner fail closed for these operations until their ownership is explicitly backfilled or the agent is re-registered. Remaining limitation: the registering Firebase user is still acting on behalf of the agent; cryptographic agent credentials and an independent agent principal are not implemented, so this is user-owner isolation rather than cryptographic agent authentication. Query-based retrieval applies SQLite scope/approval/synchronization filters before MongoDB hydration and verifies the stored hash. Immutable epochs now pin exact MongoDB document versions and hashes. Session retrieval resolves the immutable epoch from the authenticated agent session and returns only exact pinned document versions. The combined session-context endpoint joins these documents with progressive SQLite memory retrieval. Project-shared access is now backed by persistent SQLite projects and memberships. Project creation is workspace-owner-only; workspace owners manage workspace membership; project owners/admins manage member/viewer grants, while only project owners can grant elevated roles. Project-shared create, search, direct retrieval, version history, approval, and session hydration check active membership. Workspace membership revocation also revokes the user's project memberships. Scope promotion remains unimplemented.


## Approval authorization

Memory document approval is available only to an authenticated user who is currently a member of the target workspace. The current Axon role model does not yet expose granular memory-approval capabilities, so approval is workspace-member scoped; introduce role/capability checks when that authorization model is implemented.


## Reconciliation and operator actions

The workspace reconciliation endpoint is read-only and returns identifiers/status mismatch categories, never document contents. Dead-letter retry is audited, but the current API role model has no administrator capability; the retry endpoint is currently workspace-member scoped and should be restricted when role-based authorization is added.


## Project and workspace membership

Projects and memberships are persisted in SQLite. A project is bound to a workspace and its creator must be a workspace owner. Project members must also be active workspace members. The API exposes workspace-member addition/revocation and project creation/listing/member grants/revocation. Only workspace owners can manage workspace membership; project owners and admins can grant member/viewer access, but only project owners can grant owner/admin roles. Existing project owners cannot be demoted or revoked through these routes. Revoking workspace membership also revokes project memberships in that workspace. Memory authorization checks the current membership state on every project-scoped retrieval and approval; an immutable epoch pin does not bypass a later revocation.
