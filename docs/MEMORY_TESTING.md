# Memory Testing

## Local checks

Run from the repository root:

```sh
cargo fmt --all -- --check
cargo check --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

## Current unit coverage

`memory_documents.rs` includes tests for stable SHA-256 content hashing, stale-hash rejection, supported private scope, and invalid-scope rejection.

Existing QKV and memory-intelligence tests remain part of the Rust suite.

## Required integration coverage before production

- Run migrations 0010 and 0011 against a fresh SQLite database and an upgraded database.
- Verify MongoDB unique memory/version index behavior.
- Verify identical outbox replay is idempotent.
- Verify conflicting versions fail.
- Simulate MongoDB outage and worker restart.
- Simulate lease expiry and duplicate delivery.
- Verify hash mismatch never becomes retrievable.
- Verify dead-letter handling and operator recovery.
- Verify workspace/task/agent scope isolation.
- Verify immutable epochs pin exact document versions.
- Verify MongoDB documents are hydrated through QKV and ranked by Qoin.
- Run the full suite with a real isolated MongoDB instance.

The connected GitHub file-edit workflow does not provide a Rust runtime in this session. No compilation or test pass should be claimed until CI or a local checkout executes these commands.
