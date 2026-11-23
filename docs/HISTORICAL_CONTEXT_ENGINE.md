# Axon Historical Context Engine

## Contract

The immutable chat and execution event history is the source of truth. Qoin vectors are derived indexes; QKV attention ranks candidate memories; original events remain retrievable and replayable. The engine never treats an embedding, summary, or agent-generated explanation as the authoritative record.

## Data path

1. Chat messages, decisions, task transitions, artifact changes, tool results, and outcomes are appended as source events.
2. Qoin produces versioned vectors. Axon stores them with workspace scope, source event sequence, dependencies, visibility, outcome/evidence scores, and a pointer to the original payload.
3. Multi-head scaled dot-product attention compares the current task query with historical keys. Values are source references/payloads, not lossy replacements.
4. A relevance and evidence ranking is computed. The initial context uses a strict 20% budget and attempts to cover 80% of ranked contribution. If it cannot, Axon reports the actual coverage and defers the rest; it never claims a universal 80/20 law.
5. The rest of the memories remain available for progressive, task-local retrieval.
6. A reconstruction stage replays immutable source events and compares the result against an independent checkpoint before context is marked verified.

## Hourly consolidation and stable work

A global memory epoch is published at most once per hour per workspace (and on first initialization). Each epoch records the highest source event sequence, indexed-memory count, publication time, and digest. Epoch rows are immutable.

An agent session pins one epoch at assignment. Its initial context and later retrieval references are task-local. A subsequent epoch never mutates an existing session or rewrites the agent's working memory. New agents use the latest published epoch.

Urgent security controls, revoked permissions, deleted access, and critical invalidation are not delayed by the hourly cycle. Authorization is checked at retrieval time even for an older pinned epoch.

## QKV and Qoin responsibilities

- Qoin: embedding/vector production, model/version identity, vector normalization conventions.
- Axon: access control, vector index lifecycle, QKV attention, Pareto selection, provenance, memory epochs, agent sessions, reconstruction, and audit.
- Q = current task/context vector.
- K = versioned Qoin historical vectors.
- V = original memory payload references.
- Multi-head attention partitions the vector dimensions into heads, scales dot products by sqrt(head dimension), and averages head logits before ranking.

The initial implementation accepts Qoin vectors through a typed adapter contract. It does not fabricate embeddings or call an unconfigured provider. A model-version change requires re-indexing or explicit compatibility checks; vectors from incompatible models must not be mixed.

## Pareto policy

The default is a 20% initial context budget and an 80% contribution-coverage target. This is an operational heuristic, not a mathematical guarantee. Selection is constrained by the budget and returns achieved coverage plus a budget-exhausted flag. The deferred 80% remains indexed and can be retrieved asynchronously as task needs evolve. A low-coverage result should trigger a wider retrieval or a larger context budget, not silent omission.

## Isolation and consistency

- Workspace boundaries are mandatory.
- Agent-private memories are visible only to their owner.
- Task/project memories require matching task/project identifiers.
- Only approved, non-superseded memories are eligible.
- Sessions pin epoch IDs and immutable initial memory IDs.
- Progressive retrieval appends references to the session; it does not replace its initial context.
- Every result carries source event sequence and model version for reproducibility.

## Operations and production boundaries

The hourly publication schema and deterministic QKV/Pareto engine are included. A production deployment must run the coordinator as a supervised background task and connect the Qoin vector writer to the index contract. Do not enable a pretend embedding provider. SQLite remains the current single-node persistence backend; a multi-node deployment needs a shared durable database and a distributed lease/leader mechanism so two workers cannot publish competing epochs.

Encryption must use vetted authenticated encryption and managed key custody. Never invent a custom cipher. Treat embeddings as sensitive derived data.
