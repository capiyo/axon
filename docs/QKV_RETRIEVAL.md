# QKV Memory Retrieval

Axon already implements multi-head scaled dot-product attention in `memory_architecture.rs`.

## Query

A query is represented by a versioned `QoinVector` plus retrieval constraints such as tags, dependencies, workspace, task, project, agent, and session context.

## Key

The current epoch index stores vector keys and metadata including tags, dependency IDs, outcome/evidence values, source sequence, and model version. MongoDB document metadata provides a place to persist richer key metadata and a QKV version.

## Value

Existing QKV values are stable references/payload snapshots stored in SQLite epoch items. MongoDB stores full versioned memory documents. Session-context retrieval now combines progressive SQLite epoch values with exact MongoDB document versions pinned to the same agent session.

## Retrieval guarantees

- Query and key vector dimensions and model versions must match.
- Scope and approval filters are applied before retrieval results are returned.
- Epochs remain immutable.
- Progressive retrieval excludes initial and previously retrieved memories.
- Lexical fallback must be described as lexical/metadata retrieval, not neural semantic search.

## Remaining work

MongoDB document search uses a deterministic FNV-1a hashed lexical vector (64 dimensions, four attention heads) as the current QKV key. It is explicitly labelled `deterministic_lexical_qkv_v1`, not neural semantic search. Candidate metadata is filtered in SQLite before MongoDB hydration and hash verification. The lexical vector is passed to the existing Qoin memory scorer. New memory documents persist their 64-dimensional lexical QKV key, key version, and SHA-256 hash of the exact title/content/tag source text in MongoDB `key_metadata`; retrieval validates normalization, model version, and source hash, and uses deterministic query-time fallback only for older documents. Neural embedding-provider integration remains outstanding. The session-document retrieval endpoint resolves the epoch through the authenticated agent's `agent_memory_sessions` record (rather than accepting a caller-selected epoch), hydrates the exact `memory_id + document_version` pinned in `memory_document_epoch_items`, verifies its hash and QKV metadata against SQLite, and applies the pinned scope before returning values. The combined `POST /api/v1/workspaces/{workspace_id}/agents/{agent_id}/memory-sessions/{session_id}/context` endpoint additionally performs progressive retrieval and returns both stores in one session-bound context response. MongoDB document values are bounded to 256 KiB per response; whole documents are omitted rather than truncated and `documents_truncated` reports omissions.
