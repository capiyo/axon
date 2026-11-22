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

Both clients support Firebase Email/Password sign-in and account creation, persisted session restoration, sign-out, and automatic Firebase ID-token bearer injection. The API provisions a personal workspace on first authenticated request and enforces workspace membership server-side. The clients support workspace, agent, artifact, chat/memory, task handoff, response capture, and review workflows. Agent registration remains descriptive: it does not create a live provider session. Provider API handoffs currently run through the Python connector, not directly from the browser or mobile UI. Axum uses SQLite with versioned migrations and transactional entity/audit writes.