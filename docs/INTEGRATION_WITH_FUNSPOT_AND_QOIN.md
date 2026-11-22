# Axon with Funspot and Qoin

Axon is intended to improve the engineering workflow across the user's projects without becoming a hidden dependency that can silently alter them.

## Example: Funspot

A task might ask agents to investigate why fixture state changes to `live` too early.

A Axon workspace could hold:
- the reported behavior and reproduction steps;
- relevant frontend, backend, and scraper evidence;
- the agent that inspected each subsystem;
- competing explanations and their supporting evidence;
- a decision record identifying the selected fix;
- proposed patches and test results;
- a human acceptance record.

An agent reviewing the React Native client should receive only the relevant files, findings, and contracts—not every unrelated conversation or private artifact in the workspace.

## Example: Qoin

A Qoin workspace could coordinate work on a market-data adapter, deterministic signal logic, risk policy, or backtesting methodology.

Axon should preserve:
- data source and timestamp provenance;
- algorithm and parameter versions;
- assumptions and limitations;
- backtest configuration and reproducibility metadata;
- independent review findings;
- accepted and rejected design decisions.

A model-generated interpretation must never be presented as verified market data or a validated performance result.

## Integration boundary

The first Axon release does not require either project to depend on Axon at runtime. Start with repository links, manually imported artifacts, and user-approved context bundles. The Python bridge now supports importing a user-selected Claude export and persisting approved DeepSeek responses through the authenticated Axon artifact API. Later, add read-only repository adapters and controlled patch proposals.

Any write access to Funspot or Qoin must be separately authorized, scoped to a repository and branch, previewed, and audited. A Firebase token for Axon does not grant GitHub repository write access. Repository operations require a separate GitHub authorization and branch-scoped permission. Axon must never silently copy source code, create commits, merge branches, or deploy either project. Production deployment and secret management remain outside Axon's authority unless a future explicit execution design is approved.

## Long-term direction

Axon becomes the shared engineering memory and coordination layer:
- Funspot contributes real-world product, frontend, backend, and operations tasks.
- Qoin contributes data, quantitative research, testing, and risk-analysis tasks.
- Axon provides the common handoff, provenance, review, and decision infrastructure.

The projects remain independently buildable and deployable.


## Automatic exchange and capture

Axon supports an authenticated API bridge for explicit artifact import/export. The current Python connector can take a selected Claude export, normalize it into the Axon Protocol envelope, save it to a workspace as an artifact, and optionally send the selected context to DeepSeek after approval. The provider response can then be persisted as another artifact.

This is an API-mediated handoff, not an unrestricted clipboard agent. Browser-assisted capture should be implemented as a user-invoked extension action: the user selects the exact conversation or text, previews the payload and target workspace, then confirms transfer. Clipboard access must be limited to a direct user gesture and must never run as a background watcher. Provider session cookies, browser storage, private endpoints and hidden DOM scraping are prohibited.

See [the Python connector guide](../connectors/python/README.md) for setup and commands.
