# Axon Python connector

The Python connector is the provider-adapter boundary for explicit, inspectable AI context handoffs.

## Claude / ChatGPT export → Axon → DeepSeek

1. Export your data from Claude or ChatGPT using the provider's supported export controls.
2. Select one conversation JSON file. For ChatGPT's `conversations.json` containing multiple conversations, specify the conversation ID.
3. Run the connector in preview mode. It reports source, message count and context size without sending content.
4. Use `--print-context` to inspect the exact normalized AXP envelope before transfer. Treat this output as private conversation data.
5. Use `--import-to-axon` to explicitly save the selected source conversation as an Axon artifact.
6. Add `--approve-transfer` to send that selected context to DeepSeek through its API.
7. Add `--persist-response` to save the approved DeepSeek response as a second Axon artifact.

Claude's consumer chat history is not exposed by the standard Anthropic Messages API. This implementation therefore imports user-authorized export files; it does not scrape logged-in browser pages, inspect cookies, automate private endpoints, or bypass provider controls. ChatGPT imports parse the exported conversation mapping and preserve ordered text messages; multimodal attachments and tool payloads are not yet transferred.

## Setup

Requires Python 3.11+.

```bash
cd connectors/python
python -m venv .venv
# Windows: .venv\\Scripts\\activate
# macOS/Linux: source .venv/bin/activate
pip install -e ".[dev]"
```

Set provider and Axon credentials in the environment. Do not put them in source code or commit them.

```powershell
$env:DEEPSEEK_API_KEY="your-deepseek-key"
$env:AXON_API_URL="https://your-axon-api"
$env:AXON_FIREBASE_ID_TOKEN="your-short-lived-firebase-id-token"
```

For local development, `AXON_API_URL=http://127.0.0.1:8080` is permitted. Remote API endpoints must use HTTPS. The Firebase ID token is sent only as a bearer token to the configured Axon API endpoint; never send it to a model provider.

## Preview a conversation

```bash
axon-bridge ./conversation.json \
  --workspace-id YOUR_WORKSPACE_UUID \
  --actor-id local-import \
  --instruction "Independently review the architecture and identify risks." \
  --print-context
```

Preview is the default. No provider request or Axon write is made without an explicit command-line flag. The context preview can contain private conversation data; do not paste it into public logs or tickets.

## Import the selected conversation into Axon

```bash
axon-bridge ./conversation.json \
  --workspace-id YOUR_WORKSPACE_UUID \
  --actor-id local-import \
  --instruction "Review this conversation." \
  --import-to-axon
```

The connector calls the authenticated `POST /api/v1/workspaces/{workspace_id}/artifacts` endpoint. Axon verifies the Firebase ID token and derives the actor and workspace membership server-side. The connector's `--actor-id` value is provenance metadata only; it is not an identity claim and is not sent as authorization.

## Approve a provider handoff and persist the response

```bash
axon-bridge ./conversation.json \
  --workspace-id YOUR_WORKSPACE_UUID \
  --actor-id local-import \
  --instruction "Independently review the architecture and identify risks." \
  --approve-transfer \
  --persist-response
```

`--persist-response` requires `--approve-transfer` and a Firebase ID token. It stores the provider response as an Axon artifact only after the explicit transfer approval and successful provider response. The source conversation is stored separately only when `--import-to-axon` is also supplied.

## API bridge contract

- Uses the existing Axon authenticated artifact API; no new password or API-key endpoint is introduced.
- Requires a Firebase ID token from an Axon-authenticated user and server-side workspace membership.
- Remote URLs must use HTTPS. Plain HTTP is accepted only for localhost development.
- Workspace IDs are validated as UUIDs before constructing a request path.
- Artifact titles and content are validated against Axon's current limits (200 title characters; 512,000 content bytes).
- Authorization tokens and provider response bodies are never included in connector error messages.
- The connector does not silently monitor the clipboard. A browser extension or clipboard integration must be a separate, visible user action with an explicit capture scope and revocation.

## Current boundaries

- Claude and ChatGPT user-authorized export imports; no browser scraping or direct Claude consumer-chat-history API.
- DeepSeek API call is implemented.
- Axon artifact persistence is implemented through the authenticated API bridge.
- AXP context payload includes a SHA-256 digest for integrity checking; this digest detects payload changes but is not a signature or proof of origin.
- Attachments, images, tool-use blocks and very large conversations are not yet transferred.
- The API bridge uses a short-lived Firebase ID token supplied by the signed-in user; automated long-running service integrations need a separate, scoped credential design.
