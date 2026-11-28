# Google Sign-In Setup

Axon presents Google Sign-In as its only end-user sign-in method. The Google account is selected through the Google OAuth provider; Firebase Authentication is retained only as the client-side identity-token/session bridge for the existing Axon API verifier. Axon's API never receives Google passwords or OAuth client secrets.

## Firebase project

Use one dedicated Firebase project for Axon.

1. Register a Web app and copy its public SDK configuration.
2. Register the Android app with package name `co.axon.mobile`.
3. Register the iOS app with bundle identifier `co.axon.mobile`.
4. In Firebase Authentication, enable the Google provider and configure the project's support email. Do not enable Email/Password as an Axon sign-in option.
5. Add the deployed web hostname to Firebase Authentication's authorized domains.
6. Configure the Google provider and project-level abuse protections before production use.

## Web client

Create `frontend/apps/web/.env.local` from `.env.example`:

```dotenv
VITE_FIREBASE_API_KEY=
VITE_FIREBASE_AUTH_DOMAIN=
VITE_FIREBASE_PROJECT_ID=
VITE_FIREBASE_APP_ID=
```

The web client uses the Google provider through the Firebase SDK popup flow. Add the deployed hostname to the Firebase Authentication authorized domains and the matching Google OAuth web client configuration in Google Cloud.

These are Firebase client SDK configuration values, not service-account credentials. Never put a Firebase Admin SDK private key or service-account JSON in a Vite variable.

## Mobile client

Create `frontend/apps/mobile/.env` from `.env.example`:

```dotenv
EXPO_PUBLIC_AXON_API_URL=http://127.0.0.1:8080
EXPO_PUBLIC_FIREBASE_API_KEY=
EXPO_PUBLIC_FIREBASE_AUTH_DOMAIN=
EXPO_PUBLIC_FIREBASE_PROJECT_ID=
EXPO_PUBLIC_FIREBASE_APP_ID=
EXPO_PUBLIC_GOOGLE_WEB_CLIENT_ID=
EXPO_PUBLIC_GOOGLE_ANDROID_CLIENT_ID=
EXPO_PUBLIC_GOOGLE_IOS_CLIENT_ID=
```

Register OAuth 2.0 client IDs for the web, Android (`co.axon.mobile` plus its signing certificate SHA-1), and iOS (`co.axon.mobile`) applications. The mobile app uses Expo AuthSession with `expo-crypto` (PKCE) to obtain a Google ID token and exchanges it for a Firebase session credential. The registered app URL schemes include both `axon` and `co.axon.mobile`; the latter matches Expo AuthSession's native OAuth callback (`co.axon.mobile:/oauthredirect`). Configure the matching native OAuth client IDs and signing fingerprints before building a release. If the active platform's client ID is missing, the app displays a setup message rather than attempting a broken sign-in.

For an Android emulator, use `http://10.0.2.2:8080`. For a physical device, use a reachable HTTPS API endpoint or the documented USB reverse setup. Expo public variables are embedded in the client bundle and must never contain private keys.

## API

Set the same Firebase project ID on the Axon API:

```dotenv
FIREBASE_PROJECT_ID=
AXON_DATABASE_URL=sqlite://axon.db
AXON_ENABLE_SIMULATOR=false
```

The API verifies Firebase ID tokens against Google's Secure Token JWKS and checks issuer, audience, signature, and expiry. It provisions an internal user, personal workspace, owner membership, and audit event transactionally on first authenticated request.

Only `GET /health` and `GET /ready` are public. Every registered `/api/v1` route requires `Authorization: Bearer <Firebase ID token>`. Workspace and task identifiers are checked against the authenticated user's memberships; cross-user access returns 404.

## Current provider scope

The web and mobile entry screens now expose Google Sign-In only. Email/password registration, password reset, and email verification are not part of the Axon sign-in experience. Firebase ID tokens are still used between the clients and Axon's API; this is an internal token-verification detail, not a separate user-facing sign-in choice. Invitations and workspace role management are also outside this authentication slice.
