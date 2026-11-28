# AXON Python connector

The Python connector provides explicit, inspectable context handoffs from user-authorized provider exports into DeepSeek and Axon's authenticated artifact API.

## Supported imports

- Claude: one conversation JSON export.
- ChatGPT: one conversation object or `conversations.json`; when multiple conversations are present, select one with `--conversation-id`.

The connector does not scrape logged-in browser pages, inspect cookies, automate private endpoints, or bypass provider controls.

## Setup

Requires Python 3.11+.

```bash
cd connectors/python
python -m venv .venv
# Windows: .venv\\Scripts\\activate
# macOS/Linux: source .venv/bin/activate
pip install -e ".[dev]"
```

Set the DeepSeek API key only when you intend to approve a provider transfer:

```powershell
$env:DEEPSEEK_API_KEY="your-key"
```

```bash
export DEEPSEEK_API_KEY="your-key"
```

## Preview a Claude conversation

```bash
axon-bridge ./conversation.json \
  --workspace-id YOUR_WORKSPACE_ID \
  --actor-id PROVENANCE_LABEL \
  --instruction "Independently review the architecture and identify risks." \
  --print-context
```

## Preview a ChatGPT conversation

```bash
axon-bridge ./conversations.json \
  --provider chatgpt \
  --conversation-id YOUR_CONVERSATION_ID \
  --workspace-id YOUR_WORKSPACE_ID \
  --actor-id PROVENANCE_LABEL \
  --instruction "Independently review the architecture and identify risks."
```

Preview is the default. No provider request is made without `--approve-transfer`. The exact normalized AXP envelope can be inspected with `--print-context`; treat it as private conversation data.

## Explicitly save source context to Axon

Set the API origin and a Firebase ID token for a user who is authorized in the target workspace:

```powershell
$env:AXON_API_URL="https://your-axon-api.example"
$env:AXON_FIREBASE_ID_TOKEN="your-firebase-id-token"
```

```bash
export AXON_API_URL="https://your-axon-api.example"
export AXON_FIREBASE_ID_TOKEN="your-firebase-id-token"
```

Then explicitly import the selected conversation as an Axon artifact:

```bash
axon-bridge ./conversations.json \
  --provider chatgpt \
  --conversation-id YOUR_CONVERSATION_ID \
  --workspace-id YOUR_WORKSPACE_ID \
  --actor-id PROVENANCE_LABEL \
  --instruction "Review this context." \
  --import-to-axon
```

The API client requires HTTPS except for localhost development, sends the Firebase ID token as a bearer token, validates workspace UUID and artifact bounds, and does not treat `--actor-id` as authentication. The backend derives the authenticated actor from the verified token.

## Approve a DeepSeek handoff and optionally persist its response

Add `--approve-transfer` to make the provider request. To save the returned response as a separate Axon artifact, also add `--persist-response`:

```bash
axon-bridge ./conversation.json \
  --workspace-id YOUR_WORKSPACE_ID \
  --actor-id PROVENANCE_LABEL \
  --instruction "Review the plan and identify concrete risks." \
  --approve-transfer \
  --persist-response
```

Response persistence requires `AXON_FIREBASE_ID_TOKEN` (or `--axon-id-token`) and an authorized workspace. The connector does not silently trigger billable provider calls.

## Configuration

- `DEEPSEEK_API_KEY`: required for approved DeepSeek transfers.
- `DEEPSEEK_MODEL`: optional model override; defaults to `deepseek-chat`.
- `AXON_API_URL`: optional Axon API origin; defaults to `http://127.0.0.1:8080` for local development.
- `AXON_FIREBASE_ID_TOKEN`: Firebase ID token for explicit artifact import or response persistence. Prefer the environment variable to avoid shell-history exposure.
- `--max-context-chars`: bounds the serialized context; defaults to 120,000 characters.

## Security and current boundaries

- Source exports are user-supplied and explicitly selected; no private browser session access or provider scraping.
- Preview does not contact DeepSeek. `--approve-transfer` is required for provider execution.
- Axon artifact writes require a verified Firebase ID token and server-side workspace authorization.
- The AXP SHA-256 digest detects payload changes; it is not a signature or proof of origin.
- Attachments, images, tool-use blocks and very large conversations are not transferred by this connector.
- Never put provider API keys or Firebase ID tokens in source code, Vite/Expo client variables, or committed files.


## Persistent memory extraction (ChatGPT or Claude)

The `axon-memory` command imports a user-authorized conversation export, asks DeepSeek to extract durable memory candidates, validates the output and source message references, and can persist the source import plus proposed memories in Axon's MongoDB-backed memory repository. Proposed memories remain inactive until reviewed.

Configure the API origin and a Firebase ID token for a member of the target workspace:

```powershell
$env:AXON_API_URL="https://your-axon-api.example"
$env:AXON_FIREBASE_ID_TOKEN="your-firebase-id-token"
$env:DEEPSEEK_API_KEY="your-deepseek-key"
```

```bash
export AXON_API_URL="https://your-axon-api.example"
export AXON_FIREBASE_ID_TOKEN="your-firebase-id-token"
export DEEPSEEK_API_KEY="your-deepseek-key"
```

Preview a ChatGPT conversation without contacting a provider or writing data:

```bash
axon-memory ./conversations.json --provider chatgpt --conversation-id CHAT_ID --workspace-id WORKSPACE_UUID
```

Approve extraction and inspect proposed candidates:

```bash
axon-memory ./conversations.json --provider chatgpt --conversation-id CHAT_ID --workspace-id WORKSPACE_UUID --approve-provider-transfer
```

Approve both provider transfer and persistence:

```bash
axon-memory ./conversations.json --provider chatgpt --conversation-id CHAT_ID --workspace-id WORKSPACE_UUID --approve-provider-transfer --approve-persist
```

Persistence requires `AXON_FIREBASE_ID_TOKEN` and workspace membership. The source export is stored as a conversation import; extracted records are stored with `proposed` status and are not active context until reviewed through Axon's memory review endpoint. The API rejects source-message IDs that do not belong to the imported conversation.

## Multi-model capability majority routing

The `axon-route` command asks configured OpenAI-compatible models to nominate a specialist, persists a workspace-scoped capability poll, and executes the task only on a strict-majority winner. Configure up to 30 models in a private copy of `models.example.json`. Each entry includes its provider, endpoint, capability tags, and the environment variable containing its API key. Never put provider keys in the JSON file.

Set each model's key environment variable, plus `AXON_API_URL` and `AXON_FIREBASE_ID_TOKEN`. Preview does not send the task to a provider or create a poll:

```bash
axon-route --task "Design a responsive dashboard" --capability visual_design --workspace-id WORKSPACE_UUID --models ./models.json
```

After reviewing the eligible voters, explicitly approve provider sharing:

```bash
axon-route --task "Design a responsive dashboard" --capability visual_design --workspace-id WORKSPACE_UUID --models ./models.json --approve-provider-sharing
```

Use `--max-cost-per-1k-tokens` to impose an estimated USD cost ceiling. Use `--include-memory` to retrieve active workspace memories relevant to the task; selected memory context is also shared with eligible voters after explicit approval. Provider keys remain in environment variables, while Axon API writes and reads use the Firebase bearer token and enforce workspace membership. A tie or plurality does not select a winner; only a strict majority dispatches the task to the selected model.

The backend requires MongoDB configuration for durable memory imports and capability polls. Keep provider credentials and Firebase ID tokens out of source files, shell history, and public logs.
