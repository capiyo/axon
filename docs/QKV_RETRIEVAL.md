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

Implement MongoDB value hydration, a versioned embedding-provider interface, deterministic fallback candidate generation, and tests that pin exact MongoDB document versions into epochs.
