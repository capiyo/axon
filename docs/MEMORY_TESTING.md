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

`memory_documents.rs` includes tests for stable SHA-256 content hashing, stale-hash rejection, supported private scope, and invalid-scope rejection. `memory_outbox.rs` tests idempotent enqueue, transactional approval audit, and private-agent scope filtering. `memory_vector_provider.rs` tests deterministic lexical QKV vector generation and normalization. The outbox integration test also verifies immutable document-version/hash/QKV/Qoin epoch pins.

Existing QKV and memory-intelligence tests remain part of the Rust suite.

## Required integration coverage before production

- Run migrations 0010, 0011, and 0012 against a fresh SQLite database and an upgraded database.
- Verify MongoDB unique memory/version index behavior.
- Verify identical outbox replay is idempotent.
- Verify conflicting versions fail.
- Simulate MongoDB outage and worker restart.
- Simulate lease expiry and duplicate delivery.
- Verify hash mismatch never becomes retrievable.
- Verify dead-letter handling and operator recovery.
- Verify workspace/task/agent scope isolation.
- Verify a non-member cannot approve a proposed memory document.
- Verify the combined session-context endpoint returns progressive SQLite memories and the exact MongoDB versions pinned to that same session.
- Verify a session belonging to another agent cannot be used, and task reassignment fails closed.
- Verify the document context byte budget omits whole documents and reports truncation. Unit tests in `memory_routes.rs` cover both fitting and truncating document sets.
- Run the full suite with a real isolated MongoDB instance.

The connected GitHub file-edit workflow does not provide a Rust runtime in this session. No compilation or test pass should be claimed until CI or a local checkout executes these commands.
