# Axon agent runtime configuration

Axon keeps provider credentials on the API server. Agent records contain a provider identifier only; they never contain API keys, private endpoints, or bearer tokens.

## Supported provider protocols

The task runtime currently supports:

- Anthropic Messages API for provider identifiers containing `claude` or `anthropic`.
- OpenAI Chat Completions API for all other provider identifiers, including OpenAI-compatible gateways and self-hosted inference endpoints.

The provider identifier is normalized to uppercase ASCII letters and digits when resolving environment variables. For example, the workspace provider value `claude` resolves the `AXON_AGENT_CLAUDE_*` variables; `openai` resolves `AXON_AGENT_OPENAI_*`.

## Server environment

For the default Claude provider:

```dotenv
AXON_AGENT_CLAUDE_BASE_URL=https://api.anthropic.com/v1
AXON_AGENT_CLAUDE_API_KEY=<server-side-secret>
AXON_AGENT_CLAUDE_MODEL=<enabled-Anthropic-model-id>
```

For OpenAI:

```dotenv
AXON_AGENT_OPENAI_BASE_URL=https://api.openai.com/v1
AXON_AGENT_OPENAI_API_KEY=<server-side-secret>
AXON_AGENT_OPENAI_MODEL=<enabled-OpenAI-model-id>
```

For a self-hosted OpenAI-compatible endpoint, set the provider identifier on the agent to a stable name such as `local`, then configure `AXON_AGENT_LOCAL_BASE_URL`, `AXON_AGENT_LOCAL_API_KEY`, and `AXON_AGENT_LOCAL_MODEL`. The base URL should end at the API version prefix (for example `/v1`); Axon appends the protocol endpoint.

Set these values in the backend hosting platform's secret/environment configuration. Do not put them in frontend `.env` files, agent descriptions, database records, source control, task payloads, or audit metadata. Use a restricted provider key and rotate it according to your provider's policy.

## Execute a task

The authenticated workspace task endpoint is:

```http
POST /api/v1/tasks/{task_id}/execute
Authorization: Bearer <Firebase ID token>
Content-Type: application/json
```

An empty JSON object is valid. If a current Qoin vector snapshot is available in the web workspace, the client can supply it for historical-memory retrieval:

```json
{
  "memory_query": {
    "values": [0.1, -0.2, 0.3, 0.4, 0.0, -0.1],
    "model_version": "the-snapshot-model-version"
  },
  "heads": 2,
  "tags": ["symbol:BTCUSDT"],
  "progressive_limit": 10
}
```

The vector values above are illustrative only. Clients must send the actual Qoin snapshot vector and its exact model version. Axon never fabricates an embedding or substitutes a current market vector for historical event-time context.

When a compatible hourly memory epoch exists, Axon applies its QKV attention and Pareto initial-context selection, then optionally fetches the requested bounded progressive context. If no compatible epoch exists, execution continues with authorized chat and artifact context and records a warning in server logs.

The runtime loads approved memory entries and a bounded recent chat transcript when the task references a source chat. It marks retrieved conversation/artifact material as untrusted reference data in the provider system context.

## Lifecycle and failure behavior

- Task creation does not invoke a paid provider.
- The user explicitly starts execution from the task card.
- Axon authorizes workspace membership and confirms that the assigned agent belongs to the same workspace.
- The task is durably moved to `in_progress` before the provider call.
- A successful provider response is saved as an artifact and moves the task to `submitted`; the response is also appended to the source chat when one is attached.
- A provider timeout or error leaves the task `in_progress` and retryable. A durable SQLite lease with an owner token prevents concurrent execution across API instances sharing the same database; the 240-second lease is released on completion and can be reclaimed after expiry.
- The task still follows the existing review lifecycle; provider output is not automatically accepted or marked completed.

Axon sends a stable idempotency key derived from task ID and task version to the provider on retries. Providers that honor idempotency keys can deduplicate a retry after a process interruption. A provider that ignores the header may still perform duplicate work if the process crashes after the provider completes but before Axon persists the response. The SQLite lease coordinates API instances that share the same database; a horizontally scaled deployment must use a shared durable database rather than per-instance SQLite files.
