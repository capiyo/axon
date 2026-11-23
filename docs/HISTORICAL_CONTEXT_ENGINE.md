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


## Runtime integration

The Axum service starts a supervised Tokio coordinator. It polls once per minute and publishes an epoch only when a workspace has no epoch yet or its latest publication is at least 3,600 seconds old. Each publication runs in one SQLite transaction and freezes the currently approved vector index into `memory_epoch_items`. A later re-index cannot rewrite an existing epoch. The published epoch, its items, the agent session, and progressive retrieval references are protected by append-only triggers.

The current implementation uses SQLite as a single-node coordinator. Multiple service replicas require a distributed lease or a single elected publisher before they share a production database.

## HTTP integration

All routes are behind the existing Firebase authentication middleware and additionally verify workspace membership.

- `POST /api/v1/workspaces/{workspace_id}/memory/index`: accepts a Qoin-produced, versioned vector for an existing approved curated memory. The source event is checked before indexing; proposed or rejected memories cannot be indexed.
- `POST /api/v1/workspaces/{workspace_id}/chats/{chat_id}/messages/{message_id}/memory-index`: indexes an original chat message while retaining the original message as the source of truth.
- `GET /api/v1/workspaces/{workspace_id}/memory/epoch`: returns the latest published epoch.
- `POST /api/v1/workspaces/{workspace_id}/agents/{agent_id}/memory-context`: builds the initial Pareto context for a registered agent. If a task is supplied, it must belong to the workspace and be assigned to that agent.
- `POST /api/v1/workspaces/{workspace_id}/agents/{agent_id}/memory-sessions/{session_id}/retrieve`: retrieves additional context from the same pinned epoch and records each retrieved memory in an append-only session retrieval table.

The Qoin index request includes the vector model version. A handover query must use the same model version; incompatible vectors are skipped rather than compared. The API deliberately does not invent embeddings or silently mix model spaces.

## Request/response behavior

The index operation records Qoin vectors and source payload references; it does not modify the original chat event. Index entries created after an epoch publication become visible to new agents only after the next hourly epoch. Existing sessions always read their frozen epoch items, with a live status join so superseded/deleted memories and revoked eligibility are not resurrected from old snapshots.

Handover returns the epoch ID, initial memory payloads, deferred candidate count, measured contribution coverage, and whether the 20% budget prevented reaching the 80% target. Progressive retrieval uses the same epoch and excludes initial and previously retrieved IDs. The initial session record and retrieval records are immutable.

## Operational limits

- The hourly worker publishes metadata and freezes both curated memory entries and original chat-message vectors; it does not run an LLM summarizer or embedding model.
- Qoin must call the curated-memory or chat-message index endpoint with a valid, versioned vector. Curated memories must be approved; original chat messages are indexed against their immutable message rows.
- The current QKV engine performs ranking and source retrieval, not automatic free-form synthesis. Agent-side synthesis must preserve memory IDs and source event sequences.
- The current SQLite vector index is a JSON-backed exact scan. It is suitable as a correct baseline, not a billion-record ANN index. At that scale, replace the index implementation with a Qoin-compatible ANN/vector store while retaining epoch IDs, source provenance, visibility filters, and immutable event replay.
- The snapshot marker is a reproducibility marker, not a cryptographic digest. Integrity hashing and authenticated encryption belong in a separately reviewed storage/KMS layer.
