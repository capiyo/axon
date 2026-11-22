# Architecture

## 1. System boundary

Axon is a collaboration control plane between people and AI agents. It stores shared work products and coordinates explicit handoffs. It does not replace the underlying model providers, silently inspect their private sessions, or assume that every agent has permission to see every workspace artifact.

The first release is a modular monolith around the orchestration domain. Splitting services before throughput or isolation requirements are measured would add operational cost without proving the core workflow.

## 2. Core domain

- **Workspace:** security and collaboration boundary for a person, project, or team.
- **Agent:** a named participant with provider, model/capability metadata, connection mode, and scoped permissions. An agent registration is not proof of a live provider connection.
- **Artifact:** captured input or output with source, workspace scope, capture time and content. Content hashing, classification enforcement and retention controls remain production gates.
- **Context bundle:** a versioned, bounded selection of artifacts and decisions prepared for a specific task or recipient.
- **Task:** a unit of work with requester, owner/recipient, objective, constraints, expected deliverable, status, and due/expiry policy.
- **Handoff:** an explicit transfer of a task and context bundle to another agent or human. It requires acknowledgement and has a traceable lifecycle.
- **Decision:** a proposed or accepted choice with rationale, alternatives, evidence references, author, status, and supersession links.
- **Feedback:** a human or agent assessment tied to a specific artifact, handoff, or decision.
- **Audit event:** append-only record of security-relevant and workflow-relevant actions.

## 3. Identity and authorization boundary

Axon distinguishes identity from authorization. Web and Expo clients authenticate directly with Firebase Email/Password and send Firebase ID tokens as bearer tokens. The Axum middleware verifies RS256 signatures against Firebase Secure Token JWKS, validates project issuer/audience and expiry, provisions an internal Axon user plus one personal workspace idempotently, and checks workspace membership at the HTTP boundary. Caller-supplied IDs, request JSON and unsigned headers are never identity proof. Capability grants and invitation-based membership management remain future work.

## 4. Initial logical topology

```text
React Web / React Native
          |
     HTTPS / API
          |
   Rust Axum application
   |       |        |
Identity Workspace  Orchestration
   |       |        |
   +--- Domain services ---+
              |
       Repository layer
              |
     SQLite (current store; transactional migrations)

Optional integration edge:
Axum <-> C++ Drogon
Only for bounded high-throughput or protocol-adapter workloads.

Optional measured utilities:
Rust <-> Zig
Only behind stable interfaces and benchmarks.
```

## 7. Responsibility boundaries

### Axum (initial system of record)
Owns authentication integration, authorization, workspace membership, agent registry, artifact metadata, context assembly, task/handoff state transitions, decision records, audit events, and API contracts.

### Drogon (not required for the first vertical slice)
May own stateless ingress, webhook normalization, or high-throughput adapter endpoints if load tests justify a separate C++ service. It must call the authoritative Axum domain API for policy-sensitive state changes. It must not maintain a second copy of authorization or task lifecycle rules.

### Zig (not required for the first vertical slice)
May implement isolated binary parsing, hashing, or other measured hot paths. No business policy or persistence ownership belongs here.

### Frontend
Web and mobile clients are views over the same API and domain model. They must not independently invent authorization rules or task state transitions.

## 8. Data and consistency

SQLite is the current persistence adapter. Versioned SQL migrations create the entity, identity, membership, chat/memory and audit tables. Entity mutations and audit writes use transactions; chat/memory events are append-only and current memory state is reconstructed by replay. SQLite is suitable for local development and a single-node deployment with a persistent volume, but production backup, restore, availability and multi-instance topology must be explicitly designed before launch. Large artifact bodies may move to object storage behind a future artifact-store abstraction.

## 9. Trust and provenance

Treat model output, imported text, filenames, and external metadata as untrusted content—not executable instructions. Preserve provenance and clearly separate:
- captured source material;
- agent-generated interpretation;
- human-verified facts;
- proposed decisions;
- accepted decisions.

Context bundles must be recipient-specific and purpose-limited. Secret-bearing artifacts should be rejected or redacted before sharing. Access checks happen at read time, not only when a bundle is created.

## 10. Integration progression

1. User-selected export import and manual handoffs.
2. Authenticated Python API bridge to persist selected context and approved provider responses as Axon artifacts.
3. Browser-assisted capture initiated by the user, with preview and confirmation; no hidden clipboard polling or session scraping.
4. Official provider APIs and webhooks where available and authorized.
5. Delegated execution only after scoped credentials, approval gates, audit, and revocation are implemented.

## 11. Initial vertical slice

Implement one workspace, two manually registered agents, artifact capture, context bundle creation, task handoff, acknowledgement, response capture, decision proposal/acceptance, and an audit timeline. This validates the central value before adding provider-specific complexity.
