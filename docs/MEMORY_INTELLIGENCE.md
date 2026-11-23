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

The memory architecture now includes:

- A deterministic Rust retrieval and historical reconstruction engine.
- Multi-head scaled dot-product QKV attention over versioned Qoin vectors.
- SQLite persistence for Qoin vector indexes, immutable hourly epochs, frozen epoch items, agent sessions, and progressive retrieval references.
- Workspace, agent-private, task-shared, and project-shared visibility filtering.
- A supervised hourly epoch publisher started by the Axum service.
- Pareto-based initial context selection with a 20% budget, 80% contribution target, and explicit achieved-coverage reporting.
- APIs for indexing approved curated memories and original chat messages, preparing agent handovers, and retrieving deferred context.
- Source event/message pointers and immediate live-index invalidation when curated memories are edited, rejected, or deleted.
- Tests for QKV ranking, Pareto selection, durable epoch publication, handover, progressive retrieval, and rejection of unapproved memories.

Qoin remains responsible for producing embeddings and sending the vector with a model version. Axon does not fabricate vectors or call an unconfigured embedding provider. The SQLite implementation performs an exact JSON-backed scan and is a correctness baseline, not a billion-record ANN index. A production billion-record deployment must replace the scan with a Qoin-compatible ANN/vector index while preserving epoch snapshots, source provenance, authorization filters, and reconstruction semantics.

The implementation does not provide KMS-backed encryption, distributed multi-replica epoch leadership, a production ANN index, or automatic LLM synthesis of retrieved context. These must not be simulated with custom cryptography, fake embeddings, or mutable summaries.

## Evaluation requirements before production

- Labelled relevant/irrelevant handover queries and recall@k / precision@k measurements.
- Tests proving private/task/project/workspace scope isolation.
- Replay tests for edited, rejected, superseded, and deleted memories.
- Tamper and duplicate-event detection.
- Embedding model/version metadata and re-indexing strategy.
- Retrieval latency and storage benchmarks at 1 GB and larger history sizes.
- KMS-backed encryption, key rotation, and access audit tests.
