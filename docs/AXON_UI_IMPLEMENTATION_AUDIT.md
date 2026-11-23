# Axon UI, UX and backend implementation report

**Audited base:** `production/phase-b-chats-memory`  
**Implementation branch:** `implementation/axon-social-workspace`  
**Report date:** 2026-11-24

## Verified architecture

- Web client: React 18, TypeScript and Vite.
- Mobile client: React Native 0.74 with Expo 51.
- Shared client contracts: `frontend/packages/contracts/src/index.ts`.
- Shared theme: `frontend/packages/theme/src/index.ts` and `tokens.css`.
- Backend authority: Rust Axum.
- Persistence: SQLite through the Axum storage adapter.
- Authentication: Firebase ID tokens; web and mobile continue forwarding the current token.
- Existing workspace-scoped services: workspaces, agents, artifacts, tasks, chats, messages, project memory, audit events and the Qoin vector-memory bridge.

## Implemented

### Shared visual system and layout

- Applied the Forest & Mint palette across shared theme tokens.
- Web desktop shell now uses the agreed profile / Chats / Messaging structure.
- Desktop Explore is accessed through the app menu.
- Mobile starts on Chats and has only Chats and Explore floating navigation destinations.
- Profile is accessible from the app header on mobile and from the desktop app menu/avatar.
- Profile now surfaces workspace identity, agent/task/conversation counts, selected-conversation memory context, execution state, API/storage health and workspace resources.

### ChatCards and Messaging

- ChatCards now render the complete last message rather than an arbitrary one-line preview.
- Cards expand naturally with message content and retain full-width mobile layout.
- Backend chat summaries now return last message, message count, unread count and per-user like state without requiring a detail request for every card.
- Added per-user read-state persistence, green unread counters, like/unlike and archive actions.
- Tasks can be explicitly linked to a source conversation. ChatCards show progress only when linked task records exist; the progress is derived from task lifecycle states.
- Desktop selects the first available conversation when chats load.
- Mobile opens Messaging from a card and returns to Chats through back navigation.
- Marking a conversation read also clears its related in-app chat notifications.

### Explore and community

- Agents remain private until a workspace member explicitly publishes a listing.
- Public listings expose only the listing title, provider label, category and description; private chats, memory and artifacts are not returned.
- Explore cards use the same full-width, dynamic-height visual treatment as ChatCards.
- Added newest / most-liked / most-discussed discovery sorting.
- Added comments, threaded replies, edit/delete ownership checks, likes, follows and reporting.
- Added report records with a pending moderation status.
- Added publisher ownership state and unpublish controls.
- Added backend tests for publishing, public listing retrieval, threaded comments, reactions and notification read state.

### Notifications

- Added a durable in-app notification inbox and unread counter.
- Notifications are generated for chat activity, Explore comments/replies/reactions/follows and task responses/status changes where another member acts on the task.
- Added mark-one-read and mark-all-read operations.
- Web and mobile expose a notification bell and inbox panel.

## Security and privacy

- Publishing is opt-in; existing private agents are not automatically exposed.
- Explore APIs return listing records only, not private workspace conversation, memory, artifact or execution data.
- Comment edits and deletes are restricted to the comment author.
- Chat read states and reactions are scoped to the authenticated user.
- Notification reads are scoped to the recipient.
- Existing Firebase authentication, workspace membership checks, task lifecycle and audit behavior remain in place.
- The Funspot repository was not modified.

## Verification

GitHub Actions run **#590** completed successfully:

| Check | Result |
|---|---|
| Rust formatting | Passed |
| Rust Clippy | Passed |
| Rust tests | Passed |
| Web typecheck and production build | Passed |
| Mobile TypeScript check | Passed |
| Python connector tests | Passed |

No local build is claimed; these results are from the repository's GitHub Actions workflow.

## Known capability boundaries

- The current agent registry still represents manually registered agents. “Start workspace chat” creates a workspace conversation; it does not claim to connect a provider runtime that is not configured.
- Chat progress appears only when tasks are explicitly linked to that conversation. Historical tasks without a source-chat link do not receive invented progress.
- Explore reports are persisted as pending. A privileged, global admin moderation console is not present because the existing identity model does not expose a moderator role.
- The in-app notification inbox is implemented; no external push-delivery provider was added in this work.
- Explore currently loads the first page of comments for each visible listing. The API supports limit/offset, but the UI does not yet expose a dedicated load-more control.

## Delivery controls

- Work is isolated on `implementation/axon-social-workspace`.
- A draft pull request is open for review.
- No merge, production deployment, Firebase setting change or Funspot repository modification was performed.
