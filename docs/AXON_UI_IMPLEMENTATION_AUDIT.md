# Axon UI and backend implementation audit

**Audited branch:** `production/phase-b-chats-memory`
**Implementation branch:** `implementation/axon-social-workspace`
**Audit date:** 2026-11-24

## Verified architecture

- Web client: React 18, TypeScript, Vite.
- Mobile client: React Native 0.74 with Expo 51.
- Shared client contracts: `frontend/packages/contracts/src/index.ts`.
- Shared theme: `frontend/packages/theme/src/index.ts` and `tokens.css`.
- Backend authority: Rust Axum.
- Persistence: SQLite through the Axum storage adapter.
- Authentication: Firebase ID tokens; web and mobile attach the current Firebase token to API requests.
- Existing workspace-scoped features: workspaces, agents, artifacts, tasks, chats, messages, project memory, audit events, Qoin vector memory bridge.
- Existing chat API: list/create/get chats, append messages, archive chats, propose and mutate project memory.
- Existing mobile client is a single vertically scrolling workspace screen, not the agreed two-tab Chats/Explore experience.

## Confirmed gaps against the approved plan

1. Web still renders a narrow decorative rail and a workspace dashboard; it does not render the required profile/chats/messaging three-column shell.
2. Web chat cards are compact title/date rows and do not show the last message, full metadata or work progress.
3. Web desktop does not automatically select the first chat after loading a workspace.
4. Mobile displays workspace setup, chats, memory, agents, artifacts and tasks in one long scroll; it has no Chats/Explore floating navigation.
5. The theme is dark navy/slate/lime rather than the approved Forest & Mint palette.
6. The profile sidebar and full-height workspace overview are not implemented.
7. No global public agent discovery/listing API was found in the inspected Axum router.
8. No public Explore comment, reply, reaction, follow or moderation API was found in the inspected Axum router.
9. Existing chat contracts do not expose unread counts, likes, participant counts, progress or a last-message summary. These must not be fabricated.
10. Existing backend agent registration is workspace-scoped and manual. A private workspace agent must not silently become a public Explore listing.
11. Existing notifications are not represented in the inspected chat API or shared frontend contracts.

## Preservation constraints

- Keep Firebase authentication and token forwarding intact.
- Keep existing workspace membership checks and private workspace boundaries.
- Keep the event-sourced project memory lifecycle and resume brief behavior intact.
- Keep Qoin vector indexing and progressive handover integration intact.
- Keep task lifecycle transitions and audit records intact.
- Do not expose private messages, memory, artifacts or task execution details through public Explore responses.
- Do not modify the Funspot repository.

## Implementation sequencing

1. Replace shared theme tokens with Forest & Mint.
2. Implement the web responsive shell and full-content chat cards using actual chat/message/task data.
3. Implement profile workspace overview using real loaded state.
4. Implement mobile Chats/Explore navigation and full-content cards.
5. Add explicit public agent listing and community domain models, storage migrations, authorization and API tests.
6. Connect Explore listings, comments, replies, reactions, saves/follows and moderation.
7. Integrate notifications only where a real event source and durable unread state exist.
8. Run web typecheck/build, mobile typecheck, Rust formatting/Clippy/tests and regression checks.

## Verification limitations at audit time

The GitHub repository connector exposes file read/write and commit operations but does not provide a local shell for running the repository's build/test commands. CI results must therefore be checked from GitHub Actions after commits; no local build or test result is claimed by this audit.
