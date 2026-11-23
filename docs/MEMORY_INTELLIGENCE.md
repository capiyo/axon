# Memory Intelligence V1

## Purpose

Memory Intelligence adds deterministic, explainable retrieval and historical-state reconstruction primitives to Axon's existing append-only chat memory system. It addresses agent handover without requiring a replacement agent to read the entire historical archive.

The source of truth remains the immutable memory-event history. A vector is a retrieval index and must never replace or become the only copy of the original event payload.

## Retrieval model

Each indexed memory is scoped to a workspace and may additionally be private to an agent, shared with a task, shared with a project, or shared with the workspace. Retrieval enforces those boundaries before scoring. Only approved, non-superseded memories are candidates.

A vector contains:

- A semantic embedding supplied by a configured embedding provider or Qoin adapter.
- Context, outcome, failure, and evidence dimensions in the closed interval [0, 1].
- Original event IDs, dependency IDs, tags, scope, status, and creation time as separately retained metadata.

The query is derived from the current assignment and its verified context. The weighted score is:

S = (0.30 semantic + 0.15 context + 0.18 dependency + 0.10 outcome
     + 0.12 failure + 0.10 evidence + 0.05 recency + 0.08 tags) / 1.08

Semantic similarity uses cosine similarity mapped to [0, 1]. Dependency and tag matching use Jaccard overlap. Recency is derived from the immutable event timestamp using a 30-day half-life-style decay curve. The score breakdown is returned for explainability.

Initial classifications are:

- Relevant: score >= 0.72
- Conditionally relevant: score >= 0.42 and < 0.72
- Irrelevant: score < 0.42

These are initial policy thresholds, not learned truths. They must be calibrated against a labelled retrieval evaluation set before production use. An irrelevant result is not deleted; it remains in history and can be relevant to a future assignment.

## Handover and reconstruction

The handover flow is:

1. Build a query from the new assignment, dependencies, current state, and agent permissions.
2. Retrieve approved memories in the same workspace and permitted scope.
3. Rank and explain the candidates.
4. Load the original event snapshots and artifacts referenced by selected memories.
5. Replay those immutable snapshots into a reconstructed state.
6. Compare the reconstruction against an independently supplied expected state or verified checkpoint.
7. If reconstruction is incomplete or mismatched, retrieve more history; do not silently fill gaps or claim a successful handover.
8. Supply the validated context bundle to the replacement agent.

The reconstruction primitive checks event identity, duplicate sequence numbers, valid lifecycle transitions, and equality with an independently captured expected state. The agent's own summary is not accepted as the expected state.

## Security and cryptography

This module does not implement custom encryption. Encryption must use established authenticated encryption at rest and managed key custody at the storage boundary. Key access is scoped to the workspace and agent authorization model, with rotation and revocation support.

Vector embeddings can leak semantic information. They must be treated as sensitive derived data, access-controlled like the source memory, and encrypted at rest. If a future deployment requires search over encrypted vectors, that is a separate cryptographic design decision; ordinary encryption does not make arbitrary vector search possible.

## Current implementation boundary

The Rust module `backend/axum/src/memory_intelligence.rs` provides:

- Validated vector and query types.
- Explainable weighted scoring.
- Workspace, agent, task, and project visibility checks.
- Approved/non-superseded filtering.
- Deterministic ranking and relevance classification.
- Append-only event replay and expected-state comparison.
- Unit tests for ranking, scope enforcement, invalid vectors, and reconstruction.

This first code slice is a pure domain engine. It does not yet persist vector embeddings, call an embedding provider, add public retrieval endpoints, encrypt data, or automatically generate a handover bundle. Those integrations must be added against Axon's existing SQLite memory event log and approved authentication model; they must not be simulated with hard-coded embeddings or custom encryption.

## Evaluation requirements before production

- Labelled relevant/irrelevant handover queries and recall@k / precision@k measurements.
- Tests proving private/task/project/workspace scope isolation.
- Replay tests for edited, rejected, superseded, and deleted memories.
- Tamper and duplicate-event detection.
- Embedding model/version metadata and re-indexing strategy.
- Retrieval latency and storage benchmarks at 1 GB and larger history sizes.
- KMS-backed encryption, key rotation, and access audit tests.
