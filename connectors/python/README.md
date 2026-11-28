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
