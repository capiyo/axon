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

It applies explicit normalized components, ranks deterministically, filters by workspace/status/scope, and returns explanations. The versioned `QoinScoringConfig` exposes the scoring version, component weights, relevance thresholds, recency half-life, and explanation thresholds. Each response includes the exact configuration and a SHA-256 effective policy fingerprint. Immutable memory epochs pin that fingerprint, so changing a coefficient without changing the human-readable label still invalidates session-policy compatibility.

## Existing attention ranking

`memory_architecture.rs` performs multi-head scaled dot-product attention over Qoin keys and combines attention with outcome, evidence, tags, and dependency overlap.

## Separation from market Qoin

`qoin_vectors.rs` calls a separate Qoin service for market vectors, including event-time historical lookups. Market strategy vectors and trading actions must not become memory relevance scores by accident.

## Default scoring policy

The default `axon-qoin-memory-v1` policy uses normalized components with weights: semantic 0.30, context 0.15, dependency 0.18, outcome 0.10, failure 0.12, evidence 0.10, recency 0.05, and tags 0.08. The sum (1.08) is normalized during scoring. These are explicit baseline coefficients, not trained parameters. Operators can supply a JSON `QoinScoringConfig` through `AXON_QOIN_MEMORY_SCORING_CONFIG`; invalid weights, thresholds, or versions stop startup.

## Remaining work

- Add evidence/source reliability and contradiction/duplication features only when their semantics and normalization are defined.
- Evaluate the versioned score configuration against a labelled memory-retrieval dataset.
- The document search endpoint applies the configured Qoin score decomposition after QKV candidate attention and returns score explanations plus the exact scoring configuration.
- Record retrieval feedback without mutating immutable historical epochs.


## Baseline limitations

The shipped deterministic baseline scores semantic-vector similarity, context, dependency overlap, outcome alignment, failure signal, evidence strength, recency, and tags, with component explanations. It does not yet model source reliability, reuse frequency, explicit contradiction detection, duplication penalties, or persisted retrieval-outcome feedback. Those features require defined provenance/feedback data and labelled evaluation; they are not simulated by placeholder coefficients.
