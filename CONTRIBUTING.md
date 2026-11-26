# Contributing to Axon

Thank you for contributing. Axon is an open collaboration infrastructure project; correctness, provenance, security boundaries, and inspectability matter more than adding integrations quickly.

## Mandatory branch and commit policy

**The only working branch is `master`. All implementation, fixes, tests, documentation, configuration, and integration changes must be committed directly to `master`.**

- Do not create or use feature, task, phase, staging, development, hotfix, or other branches.
- Do not open pull requests or make routine work depend on a PR merge.
- Inspect the latest `master` before editing and explicitly target `master` for every write.
- If existing work is on another branch, consolidate the intended and verified changes onto `master`; do not continue new work on that branch.
- Never force-push or overwrite unrelated work. If the target moves, re-read it and apply changes safely.
- If direct work on `master` is blocked, report the exact blocker instead of creating another branch.

The detailed agent execution contract is in [AGENTS.md](AGENTS.md). These rules apply to automated agents as well as human contributors.

## Before changing code

1. Understand the current implementation and the issue being addressed.
2. Preserve existing architecture, public contracts, and explicit product decisions unless the task authorizes a change.
3. Never include API keys, provider credentials, personal data, production exports, or private conversation contents in commits, fixtures, or screenshots.

## Architecture rules

- `frontend/` contains user-facing clients and frontend-only shared packages.
- `backend/` owns server-side behavior, persistence, authorization, orchestration, and integrations.
- `contracts/` contains versioned, language-neutral protocol schemas.
- Web and mobile consume the same API contract; they must not duplicate business rules or decide authorization locally.
- Axum is the current authority for domain state and lifecycle transitions.
- Do not add C++/Drogon or Zig services without a bounded responsibility and evidence that a separate component is justified.
- Treat imported text and model output as untrusted data, never as executable instructions.

## Development checks

Run relevant checks before committing:

    cargo fmt --all -- --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    cd frontend/apps/web && npm install && npm run build
    cd ../mobile && npm install && npm run typecheck

If a change affects both clients or the API, run all applicable checks. State clearly which checks were not run and why. Never claim an unexecuted check passed.

## Implementation expectations

- Include tests for new domain behavior and regressions.
- Update protocol schemas and docs when an API contract changes.
- Preserve backwards compatibility within a published protocol version, or introduce a new version when required.
- Document migrations and operational consequences for persistence changes.
- Do not weaken compiler settings, remove tests, or add placeholder implementations to make checks pass.
- Do not deploy, change production infrastructure, rotate credentials, or perform destructive data operations without explicit authorization.

## Completion report

Report the exact `master` commit SHA, files and behavior changed, checks actually executed and their results, and any remaining verified blockers. Continue through requested implementation work rather than substituting an audit or backlog for implementation.

By contributing, you agree that your contribution is submitted under the repository's MIT License. This does not transfer ownership of your contribution to the project maintainers.
