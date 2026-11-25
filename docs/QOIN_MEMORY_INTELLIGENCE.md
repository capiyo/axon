# Qoin Memory Intelligence

Axon's memory-ranking logic is distinct from Qoin's financial-market strategy service.

## Existing deterministic memory scoring

`memory_intelligence.rs` currently decomposes memory relevance into:
- Semantic similarity.
- Context similarity.
- Dependency overlap.
- Outcome similarity.
- Failure similarity.
- Evidence strength.
- Recency.
- Tag overlap.

It applies explicit normalized components under the version identifier `axon-qoin-memory-v1`, ranks deterministically, filters by workspace/status/scope, and returns explanations.

## Existing attention ranking

`memory_architecture.rs` performs multi-head scaled dot-product attention over Qoin keys and combines attention with outcome, evidence, tags, and dependency overlap.

## Separation from market Qoin

`qoin_vectors.rs` calls a separate Qoin service for market vectors, including event-time historical lookups. Market strategy vectors and trading actions must not become memory relevance scores by accident.

## Remaining work

- Version the memory score configuration.
- Add evidence/source reliability and contradiction/duplication features only when their semantics and normalization are defined.
- Expose score breakdowns consistently across document retrieval.
- Evaluate ranking against a labelled memory-retrieval dataset.
- The document search endpoint now applies the existing `retrieve_memories` score decomposition after QKV candidate attention and returns both score explanations.
- Record retrieval feedback without mutating immutable historical epochs.
