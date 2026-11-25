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

The MongoDB document read API and full agent identity verification are not yet implemented. Do not expose MongoDB content to clients until retrieval checks both SQLite authorization metadata and the pinned document hash/version.
