# Google Sign-In Setup

Axon uses Google OAuth as its sole human sign-in provider. Firebase Authentication is not used. Google verifies the account; the Axon API verifies Google's signed ID token and provisions Axon's own user and personal workspace in SQLite.

## Google Cloud configuration

Create OAuth 2.0 client IDs in one Google Cloud project:

- **Web application:** add every deployed Axon web origin to Authorized JavaScript origins.
- **Android application:** use package name `co.axon.mobile` and the SHA-1 fingerprints for debug and release signing certificates.
- **iOS application:** use bundle identifier `co.axon.mobile`.

Enable the Google Identity Services / OAuth consent configuration and publish the consent screen as required for the intended audience. Do not create Firebase Authentication providers or Firebase service accounts for Axon sign-in.

## Web environment

Set `VITE_GOOGLE_CLIENT_ID` to the Web OAuth client ID in `frontend/apps/web/.env.local`:

```dotenv
VITE_GOOGLE_CLIENT_ID=your-web-client-id.apps.googleusercontent.com
VITE_AXON_API_URL=http://127.0.0.1:8080
```

The web client uses Google Identity Services to receive a Google ID token. The token is held in session storage and sent to Axon's API as a bearer token.

## Mobile environment

Set the platform OAuth client IDs in `frontend/apps/mobile/.env`:

```dotenv
EXPO_PUBLIC_AXON_API_URL=http://127.0.0.1:8080
EXPO_PUBLIC_GOOGLE_WEB_CLIENT_ID=your-web-client-id.apps.googleusercontent.com
EXPO_PUBLIC_GOOGLE_ANDROID_CLIENT_ID=your-android-client-id.apps.googleusercontent.com
EXPO_PUBLIC_GOOGLE_IOS_CLIENT_ID=your-ios-client-id.apps.googleusercontent.com
```

The mobile app uses Expo AuthSession directly with Google and stores the Google ID token in AsyncStorage. There is no Firebase SDK or Firebase credential exchange.

## API environment

Configure every OAuth client ID whose ID tokens the API should accept:

```dotenv
GOOGLE_OAUTH_CLIENT_IDS=your-web-client-id.apps.googleusercontent.com,your-android-client-id.apps.googleusercontent.com,your-ios-client-id.apps.googleusercontent.com
AXON_DATABASE_URL=sqlite://axon.db
AXON_ENABLE_SIMULATOR=false
# Optional, comma-separated verified Google email addresses:
# AXON_PLATFORM_ADMIN_EMAILS=admin@example.com
```

The Rust API fetches Google's signing keys from `https://www.googleapis.com/oauth2/v3/certs`, validates RS256 signatures, issuer, audience, expiry and `email_verified`, then uses Google's immutable `sub` as the external identity key. User records, personal workspaces, memberships and audit events remain authoritative in SQLite.

## Existing accounts

The migration adds Google profile fields without deleting existing user or workspace rows. Existing Firebase-provider rows are retained as historical records. Because the old schema did not persist a verified email address for those users, Axon cannot safely infer which Google account owns each old Firebase UID. Existing accounts that need their old workspaces attached require an explicit, verified account-link migration; do not link accounts by an unverified email or by display name.
