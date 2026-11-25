# Memory Security

- All memory-document creation routes pass through Firebase authentication and workspace membership checks.
- Task-shared documents require a task in the same workspace.
- Agent-private documents require a registered agent in the same workspace.
- Project-shared document creation is rejected until project membership is persisted and enforceable.
- SQLite owns authorization metadata; MongoDB does not grant access.
- Agents must not receive MongoDB credentials or direct database access.
- Retrieved memory is untrusted content and must never override system or developer instructions.
- MongoDB connection strings and Firebase tokens must not be logged.
- Document content is not logged by the outbox worker.
- Document payloads are bounded by the API request limit and an explicit 256 KiB content limit.

Direct retrieval by memory ID/version checks SQLite workspace, scope, approval, and synchronization metadata before fetching the MongoDB version and verifying its content hash. Project-shared retrieval remains disabled. Agent identity is currently the registered agent ID in an authenticated workspace request; cryptographic agent credentials are not yet implemented. Query-based retrieval applies SQLite scope/approval/synchronization filters before MongoDB hydration and verifies the stored hash. Immutable epochs now pin exact MongoDB document versions and hashes. Session retrieval constrained to a pinned epoch remains outstanding.


## Approval authorization

Memory document approval is available only to an authenticated user who is currently a member of the target workspace. The current Axon role model does not yet expose granular memory-approval capabilities, so approval is workspace-member scoped; introduce role/capability checks when that authorization model is implemented.
