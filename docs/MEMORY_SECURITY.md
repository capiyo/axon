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

Direct retrieval by memory ID/version checks SQLite workspace, scope, approval, and synchronization metadata before fetching the MongoDB version and verifying its content hash. Project-shared retrieval is authorized through active SQLite project membership. Agent-specific memory, handover, and session routes are now bound to `Agent.created_by_user_id`, preventing another workspace member from selecting an agent they did not register. Legacy agent records without an owner fail closed for these operations until their ownership is explicitly backfilled or the agent is re-registered. Independent agent credentials are now supported for the agent's memory routes. Credentials are high-entropy bearer secrets stored only as SHA-256 hashes, expire after 90 days, and support atomic rotation and revocation. Middleware binds a credential to one workspace and one agent and rejects its use on other agents or non-agent routes. Credential-management endpoints remain human-owner-only. Query-based retrieval applies SQLite scope/approval/synchronization filters before MongoDB hydration and verifies the stored hash. Immutable epochs now pin exact MongoDB document versions and hashes. Session retrieval resolves the immutable epoch from the authenticated agent session and returns only exact pinned document versions. The combined session-context endpoint joins these documents with progressive SQLite memory retrieval. Project-shared access is now backed by persistent SQLite projects and memberships. Project creation is workspace-owner-only; workspace owners manage workspace membership; project owners/admins manage member/viewer grants, while only project owners can grant elevated roles. Project-shared create, search, direct retrieval, version history, approval, and session hydration check active membership. Workspace membership revocation revokes non-owner project memberships and agent credentials for that workspace; active workspace membership is also required by every project authorization query, including for retained project-owner rows. Scope promotion remains unimplemented.


## Approval authorization

Memory document approval is available only to an authenticated user who is currently a member of the target workspace. The current Axon role model does not yet expose granular memory-approval capabilities, so approval is workspace-member scoped; introduce role/capability checks when that authorization model is implemented.


## Reconciliation and operator actions

The workspace reconciliation endpoint is read-only and returns identifiers/status mismatch categories, never document contents. Dead-letter retry is audited, but the current API role model has no administrator capability; the retry endpoint is currently workspace-member scoped and should be restricted when role-based authorization is added.


## Project and workspace membership

Projects and memberships are persisted in SQLite. A project is bound to a workspace and its creator must be a workspace owner. Project members must also be active workspace members. The API exposes workspace-member addition/revocation and project creation/listing/member grants/revocation. Only workspace owners can manage workspace membership; project owners and admins can grant member/viewer access, but only project owners can grant owner/admin roles. Existing project owners cannot be demoted or revoked through these routes. Revoking workspace membership also revokes project memberships and agent credentials in that workspace. Memory authorization checks the current membership state on every project-scoped retrieval and approval; an immutable epoch pin does not bypass a later revocation.


## Lifecycle authorization

Archive, restore, and logical deletion require active workspace membership. Project-shared transitions additionally require active project membership; agent-private transitions require the authenticated user to own the referenced agent in the authoritative SQLite entity store. The transition checks the requested version is the latest synchronized version, validates the exact MongoDB document against SQLite metadata, and creates a new version through the transactional outbox. Tombstones hide older approved versions from normal retrieval without rewriting existing epoch pins. Current authorization is re-evaluated when historical session context is hydrated.

The delete endpoint is a logical tombstone only. It does not physically remove MongoDB content, outbox history, or immutable epoch references.


## Agent principal credentials

A workspace agent owner can issue a credential with `POST /api/v1/workspaces/{workspace_id}/agents/{agent_id}/credentials`, rotate it with `POST /api/v1/workspaces/{workspace_id}/agents/{agent_id}/credentials/{credential_id}/rotate`, and revoke it with `DELETE /api/v1/workspaces/{workspace_id}/agents/{agent_id}/credentials/{credential_id}`. The secret is returned only once at issue/rotation, is high entropy, and only its SHA-256 hash is stored. Credentials expire after 90 days. Rotation revokes the old credential and inserts the replacement in one transaction.

Agent bearer credentials are accepted only on the matching agent's memory-document, memory-session, and memory-context routes. The middleware verifies the credential's workspace and agent binding, current owner/workspace membership, and current agent registration. A credential for one agent cannot be used against another agent, workspace-management routes, or credential-management routes. The existing human Firebase authentication path remains required for credential issuance, rotation, and revocation.

The agent credential currently carries a fixed memory-route capability boundary; the broader general-purpose capability-grant model in `identity.rs` is not yet persisted or enforced for all Axon resources.


Retrieval quality feedback is restricted to the authenticated human owner; an agent bearer credential cannot submit helpful/irrelevant/contradicted/incorrect quality events. Feedback is bound to an exact version in the agent's immutable session epoch, attributed to the human user, and blended conservatively into future Qoin ranking (default weight 0.25). It is not treated as verified ground truth.
