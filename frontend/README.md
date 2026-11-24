# Frontend

The frontend is the client layer. It owns presentation, interaction state, accessibility, and device-specific behavior. It does not own authentication policy, workspace authorization, task lifecycle rules, persistence, provider credentials, or agent execution.

## Applications

- `apps/web`: React + TypeScript + Vite browser application.
- `apps/mobile`: React Native + Expo application for Android and iOS.
- `packages/contracts`: shared TypeScript API payload types consumed by both clients.

Both clients communicate with the same versioned Axon API. The shared TypeScript types are a client convenience; the language-neutral schemas under the repository's `/contracts` directory remain the protocol source of truth.

## Web

    cd frontend/apps/web
    npm install
    npm run dev

The Vite development server proxies `/api` and `/health` to the local Axum API at `127.0.0.1:8080`.

## Mobile

    cd frontend/apps/mobile
    npm install
    npx expo start

Set `EXPO_PUBLIC_AXON_API_URL` to an address reachable from the device. On a physical phone, `127.0.0.1` points to the phone itself, not the development computer. Configure the computer's reachable LAN address for local device testing.

## Contract rule

Do not define parallel copies of Workspace, Agent, Artifact, Task, TaskStatus, or Health in individual applications. Import these from `packages/contracts/src`. When an API payload changes, update the versioned protocol schema, shared types, both affected clients, and tests together.

Both clients share the same Axon API and Firebase ID-token authentication flow. Workspace data is persisted by the Rust Axum service through its SQLite storage adapter, with workspace membership and authorization enforced by the backend. Provider execution is task-scoped: a configured provider can execute an explicitly assigned task, while opening or sending a normal workspace chat message does not itself invoke a provider.

## Application structure

Both clients keep their platform entry point intentionally small and compose the application from screen modules:

- `screens/`: route-level and full-page compositions. `WorkspaceScreen` currently owns the workspace controller and composes the active workspace experience; it is the next extraction boundary for feature screens.
- `views/`: reusable page and workspace layout boundaries. Platform-specific implementations preserve web layout semantics and native safe-area behavior.
- `modals/`: overlays and sheets that are presented above the active screen. Profile and notification surfaces are separated from the workspace composition.
- `packages/contracts`: shared API types; backend protocol schemas remain authoritative.
- `packages/theme`: shared design tokens consumed by web and native clients.

Keep feature content out of the application entry points. Extract new screen-level views and modal bodies into their respective directories, keep API mutations in the existing workspace controller until they are moved into typed hooks/services, and preserve backend authorization and contract behavior during UI refactors. Do not duplicate domain rules between web and mobile.

See [ARCHITECTURE.md](./ARCHITECTURE.md) for the component-boundary conventions and current extraction status.
