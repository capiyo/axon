# QKV Memory Retrieval

Axon already implements multi-head scaled dot-product attention in `memory_architecture.rs`.

## Query

A query is represented by a versioned `QoinVector` plus retrieval constraints such as tags, dependencies, workspace, task, project, agent, and session context.

## Key

The current epoch index stores vector keys and metadata including tags, dependency IDs, outcome/evidence values, source sequence, and model version. MongoDB document metadata provides a place to persist richer key metadata and a QKV version.

## Value

Existing QKV values are stable references/payload snapshots stored in SQLite epoch items. MongoDB now stores full versioned memory documents, but QKV retrieval has not yet been switched to hydrate those MongoDB values.

## Retrieval guarantees

- Query and key vector dimensions and model versions must match.
- Scope and approval filters are applied before retrieval results are returned.
- Epochs remain immutable.
- Progressive retrieval excludes initial and previously retrieved memories.
- Lexical fallback must be described as lexical/metadata retrieval, not neural semantic search.

## Remaining work

MongoDB document search now uses a deterministic FNV-1a hashed lexical vector (64 dimensions, four attention heads) as a fallback QKV key. It is explicitly labelled `deterministic_lexical_qkv_v1`, not neural semantic search. Candidate metadata is filtered in SQLite before MongoDB hydration and hash verification. A versioned neural embedding-provider interface and exact MongoDB document-version epoch pins remain outstanding.
