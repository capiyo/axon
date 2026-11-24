# Frontend architecture

## Intent

Axon uses one product layout language across desktop web, mobile web and Android. Platform-specific components may differ, but the information architecture, navigation model, card metadata and interaction outcomes must remain consistent.

The application is organized by presentation responsibility:

```text
frontend/
├── apps/
│   ├── web/src/
│   │   ├── App.tsx
│   │   ├── screens/
│   │   ├── views/
│   │   └── modals/
│   └── mobile/
│       ├── App.tsx
│       ├── screens/
│       ├── views/
│       └── modals/
└── packages/
    ├── contracts/
    └── theme/
```

## Responsibilities

### Screens

Screens are top-level compositions and own the lifecycle of a destination. They coordinate data loading and state until that logic is extracted into typed hooks. The application entry point must only mount the screen/authentication boundary; it must not grow into another application implementation.

Current extraction boundary:
- Web: `apps/web/src/screens/WorkspaceScreen.tsx`
- Mobile: `apps/mobile/screens/WorkspaceScreen.tsx`

These files currently retain the existing workspace controller and feature JSX to avoid breaking working flows during the first structural pass. They are not the desired final size. The next extraction is to move Chats, Explore, Messaging, profile settings and workspace resources into focused screen/view modules while keeping their current API calls and state transitions intact.

### Views

Views are reusable presentation compositions. They receive typed data and callbacks; they do not own authentication, authorization, persistence or provider execution.

Current boundaries:
- Web workspace shell: `apps/web/src/views/WorkspaceView.tsx`
- Mobile workspace shell: `apps/mobile/views/WorkspaceView.tsx`

The web shell owns the desktop workspace landmark. The native shell owns safe-area containment. Both are deliberately thin platform adapters.

### Modals

Modals are overlays/sheets presented over the active screen. They receive typed data and callbacks and must not silently replace the current navigation destination.

Current boundaries:
- Web profile modal: `apps/web/src/modals/ProfileModal.tsx`
- Mobile profile sheet: `apps/mobile/modals/ProfileModal.tsx`
- Mobile notifications overlay: `apps/mobile/modals/NotificationsModal.tsx`
- Shared platform modal frames: `apps/web/src/modals/ModalFrame.tsx`, `apps/mobile/modals/ModalFrame.tsx`

The mobile profile experience is presented as a slide-up sheet over the active workspace rather than taking over the main screen. Native back requests close the sheet.

## Funspot parity rules

1. Desktop keeps the profile/sidebar context on the left and the active Chats/Messaging work area alongside it.
2. Mobile uses the compact app header and one floating pill navigation for Chats and Explore.
3. Messaging is opened from a chat card; it is not a separate primary navigation destination.
4. Profile and notification surfaces are overlays/sheets, not replacement pages.
5. Cards blend into the page surface, retain useful metadata and previews, and grow with their content.
6. Unread counters, archive/like actions, progress indicators and profile identity must use real API state. Never fabricate counts or progress.
7. Web and native clients use the same API contracts and theme tokens. Platform-specific layout code must not fork domain behavior.

## Refactoring safety

- Preserve all existing API paths, Firebase ID-token forwarding, workspace scoping, permissions and task lifecycle transitions.
- Use shared types from `packages/contracts`; do not recreate API payloads in screen files.
- Keep presentation props explicit and typed. Do not use `any`, broad casts or placeholder handlers to make extraction easier.
- Extract one feature boundary at a time and keep both platforms behaviorally aligned.
- Verify web production build and mobile TypeScript checks before declaring a refactor complete.

## Current status

The first structural pass is committed on `refactor/axon-funspot-screen-architecture`. Entry points now delegate to screen modules, workspace shells are isolated under `views/`, and profile/notification surfaces have dedicated modal modules. The existing workspace controllers remain large; full feature-by-feature extraction and visual QA are still outstanding. No merge or deployment is part of this refactor.
