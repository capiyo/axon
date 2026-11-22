# Axon model registry and assignment

## Administration boundary

Platform administration is separate from workspace ownership. Configure Firebase UIDs in the server-only `AXON_ADMIN_UIDS` environment variable (comma-separated). On the first authenticated request from an allowlisted UID, Axon creates a persistent `platform_admins` row and an audit event. An empty allowlist grants nobody admin access. Removing a UID from the environment immediately removes its effective admin access even if the historical row remains.

All `/api/v1/admin/*` handlers independently require the server-verified platform-admin flag. Do not expose this flag or registry payloads to ordinary clients.

## Provider configuration

Register a provider through the admin API. Configure only explicit HTTPS catalog URLs:

- `model_discovery_url`: JSON array of model metadata.
- `pricing_source_url`: optional JSON array of prices.
- `api_key_env`: optional environment-variable *name*, never the key itself.
- `discovery_enabled`: enables the daily refresh job.
- `rate_limit_per_minute`: minimum delay applied between provider refreshes.

The discovery worker fetches only these configured URLs. It does not search or crawl websites. If `api_key_env` is set, the corresponding value is read from the backend process environment and sent as a bearer token. It is never persisted or logged.

Example model catalog document:

```json
[
  {
    "model_name": "provider-model-v1",
    "abilities": ["text", "code", "long_documents"],
    "categories": ["general", "coding", "long_documents"],
    "context_length": 128000,
    "typical_latency_ms": 900,
    "regions": ["eu"],
    "version": "2026-11",
    "retired": false
  }
]
```

Example pricing document:

```json
[
  {
    "model_name": "provider-model-v1",
    "input_cost_per_million": 250000,
    "output_cost_per_million": 1000000
  }
]
```

Cost values are integer micro-units of the configured billing currency per million tokens. Keep the currency consistent across all providers in one registry.

New models are always candidates and unhealthy by default. They cannot receive user traffic until an admin has reviewed provider terms, an evaluation exists for every supported category at or above the approval threshold, and the admin explicitly approves the model with a note.

A price/version change or retirement is flagged. Approved model prices are not overwritten during discovery. The model is excluded from assignment while flagged. An admin resolution note is required; clearing a price/version flag applies the observed value explicitly. Clearing a retirement flag disables the model.

## Assignment configuration

All amounts below are integer micro-units of the billing currency.

- `AXON_TASK_BUDGET_MICROS`: per-run estimated budget; default 2,000,000.
- `AXON_DAILY_BUDGET_MICROS`: per-user daily reservation limit; default 10,000,000.
- `AXON_MIN_MODEL_HISTORY_RUNS`: minimum runs before historical statistics override curated cold-start ranking; default 10.
- `AXON_EXPLORATION_PERCENT`: low-risk Quick exploration share; default 5.
- `AXON_CURATED_MODELS_<CATEGORY>`: comma-separated model UUIDs in preferred order (four to six per category). Example: `AXON_CURATED_MODELS_CODING`. Until a category has a four-to-six model ranking (or individual models have a stored curated rank), unranked cold-start models are not assigned.

Privacy defaults to no training and no retention unless a run supplies a stricter or explicitly chosen requirement. Providers with unknown training or retention terms fail those strict filters.

Quick runs use one worker and deterministic checks. Normal runs use one checklist writer, one worker and one supervisor. Important runs use two checklist writers, two workers and two supervisors. Budget/diversity downgrades are recorded on the run and included in its assignment notice.

Daily budget reservations are per Firebase-backed Axon user and usage date, concurrency-safe, and audited in the same transaction. They are estimated reservations; actual provider execution/settlement must reconcile usage before billing is enabled.

## Evaluation set review gate

A first evaluation-set draft is in `docs/model-evaluation/draft-v1.json`. It is deliberately marked `draft_not_approved` and is not run automatically. Review and approve the cases, expected outputs and deterministic checks before connecting an evaluation runner or enabling scheduled evaluation. Evaluation results are stored by model, category, test-set version, date and cost. A score regression must flag and exclude the model until an admin clears it.

## Current integration boundary

The backend now has the provider-neutral `AiProvider` contract, typed checklist-writer/worker/supervisor interfaces, and provider-backed JSON role adapters. Concrete provider API adapters and credentials are intentionally not included. Implementations should be injected behind `AiProviderResolver`; credentials must remain in server environment configuration.

The deterministic assignment planner is injectable through `ModelAssigner`. Provider failure recovery is available as a pure fallback operation and preserves diversity and budget constraints. No external model call is made merely by registering or approving a model.
