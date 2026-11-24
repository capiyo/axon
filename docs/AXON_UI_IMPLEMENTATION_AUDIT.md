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
- Added report records with a pending moderation status, publisher-only report queues, dismiss actions and reported-comment removal controls.
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

GitHub Actions run **#605** completed successfully:

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
- Publisher moderation is scoped to the owner of each public listing. A privileged, global admin moderation console is not present because the existing identity model does not expose a moderator role.
- The in-app notification inbox is implemented; no external push-delivery provider was added in this work.
- Explore comment pagination is available through a load-more control; the first page is loaded for each visible listing.

## Delivery controls

- Work is isolated on `implementation/axon-social-workspace`.
- A draft pull request is open for review.
- No merge, production deployment, Firebase setting change or Funspot repository modification was performed.

## Follow-up source audit — navigation and remaining UI parity

This section records a source-level inspection of the web and React Native implementations on the branch head. It distinguishes a floating navigation control from individual floating action buttons.

### Navigation: what is actually in the code

| Platform | Verified implementation | Status |
|---|---|---|
| Web, viewport <= 900px | A centered, fixed-position, rounded pill at the bottom of the viewport. It contains Chats and Explore, has an active-state surface, and includes the aggregate unread-chat count. | Implemented |
| React Native / Android UI source | A centered, 258px-wide rounded pill with Chats and Explore pressables. It is absolutely positioned 12px from the bottom of the mobile root and has an elevated surface. | Implemented, with polish gaps |
| Desktop web | Profile sidebar and Chats/Messaging workspace are present; Explore is selected through the application menu. | Implemented |
| Mobile profile access | The app header has notification and circular profile controls. | Implemented |

**Important clarification:** Chats and Explore are implemented as two destinations inside one floating pill navigation. They are **not two separate circular floating-action buttons (FABs)**. The React Native implementation uses absolute positioning inside the root view rather than a viewport-fixed control; it does not currently apply a safe-area inset to the bottom offset. The web version does account for the safe-area inset.

The mobile header source contains Axon branding/workspace identity, notifications and profile access. It does not currently expose a separate hamburger/menu button. The profile avatar is the direct profile entry point.

### Feature-by-feature implementation matrix

| Requirement | Web | React Native / Android source | Backend | Audit result |
|---|---|---|---|---|
| Forest & Mint theme | Yes | Yes, shared tokens | N/A | Implemented |
| Desktop profile / Chats / Messaging shell | Yes | N/A | N/A | Implemented |
| Desktop Explore through menu | Yes | N/A | Yes | Implemented |
| Mobile initial Chats destination | Yes | Yes | Yes | Implemented |
| Floating Chats + Explore navigation | Fixed bottom pill | Absolute bottom pill | N/A | Implemented; native safe-area polish remains |
| Mobile app header and profile entry | Yes | Yes | Firebase identity | Implemented |
| Messaging as a child of a chat, not a navigation tab | Yes | Yes | Chat APIs | Implemented |
| ChatCard full last-message preview | No fixed line clamp in card source | No fixed line clamp in card source | Last-message summary | Implemented |
| ChatCard dynamic height and full-width mobile layout | Yes | Yes | N/A | Implemented |
| Chat title, last activity, agent/sender label and message count | Yes | Yes | Summary fields | Implemented |
| Unread counters and read-state persistence | Yes | Yes | Yes | Implemented |
| Like and archive actions | Yes | Yes | Yes | Implemented |
| Progress based on tasks linked to a chat | Yes | Yes | Yes | Implemented, only for explicitly linked tasks |
| Real agent/profile portrait in every ChatCard | Initial/avatar treatment is not a verified per-agent profile-image integration | Initial/avatar treatment is not a verified per-agent profile-image integration | No public agent-avatar contract verified | Partial |
| Explore publishing, sorting, likes, follows, comments, replies and reports | Yes | Yes | Yes | Implemented |
| Comment edit/delete and load-more pagination | Yes | Yes | Yes | Implemented |
| Publisher-scoped report review and comment hiding | Yes | Yes | Yes | Implemented |
| Global moderator role and moderation console | No | No | No role/authorization model | Not implemented |
| Durable in-app notifications and destination routing | Yes | Yes | Yes | Implemented |
| External push delivery (FCM/APNs) | No | No | No provider integration in this change | Not implemented |
| Messaging attachments / upload composer | No attachment workflow verified in the current messaging composer | No attachment workflow verified in the current messaging composer | No attachment workflow added by this PR | Not implemented in this change |
| Qoin memory indexing from message UI | Yes | Yes | Existing Qoin bridge | Implemented |
| Native Android APK/device build | Not applicable | TypeScript check only was recorded | N/A | Not verified by CI run #605 |
| External agent runtime invocation from Explore | No runtime invocation | No runtime invocation | Listing start creates a workspace chat only | Not implemented |

### Remaining work / explicit boundaries

1. **Navigation polish:** keep the existing floating pill concept. Align React Native bottom placement with device safe-area insets and verify that it never overlaps the system gesture area or composer. Do not describe the current control as two separate FABs.
2. **Agent identity imagery:** wire a real, authorized agent avatar/profile-image field end-to-end if the product contract requires actual agent portraits. Until then, the current initials/avatar treatment is the truthful fallback.
3. **Messaging attachments:** add an authenticated upload contract, storage lifecycle, message attachment metadata, and web/mobile composer UI before claiming attachments are supported.
4. **Global moderation:** introduce a privileged moderator identity/authorization mechanism and a cross-publisher moderation queue. Publisher-only moderation is not global moderation.
5. **Push notifications:** add and configure an actual delivery provider and device-token lifecycle. Current notifications are durable in-app records only.
6. **Agent runtime:** connect published-agent conversation creation to a configured execution/provider runtime; current behavior only creates a workspace chat.
7. **Android verification:** run the actual native Android build and, where available, a device/emulator smoke test. The successful mobile TypeScript check is not evidence of a successful APK build.
8. **Visual QA:** inspect real web breakpoints and Android devices for pill positioning, safe areas, card height, profile navigation and keyboard/composer behavior. CI typechecking/builds do not establish pixel-level parity with Funspot.

### Audit method and evidence

- Inspected the branch-head source files: frontend/apps/web/src/App.tsx, frontend/apps/web/src/styles.css, frontend/apps/mobile/App.tsx, and frontend/packages/theme/src/tokens.css.
- The web floating pill is rendered by the mobile-bottom-nav navigation element and styled as fixed/centered at the bottom under the 900px breakpoint.
- The React Native pill is rendered by mobileBottomNav and styled with absolute positioning, bottom 12, width 258 and rounded corners.
- Existing successful CI evidence remains GitHub Actions run #605: Rust formatting, Clippy, Rust tests, web typecheck/build, mobile TypeScript check and Python connector tests passed.
- This is a source audit, not a claim that a live browser/device visual test was performed.
