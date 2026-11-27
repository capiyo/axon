# Axon Supervisor Engine

## Purpose

Axon treats completion as a claim that must be independently checked, not as a statement made by the worker that performed the task. The supervisor engine is deterministic policy code in the Rust/Axum authority. Model providers may propose checklists and reviews, but they cannot change the merge, retry limit, evidence requirements, or final state rules.

## Execution contract

A supervisor run is designed around this sequence:

1. Two independent checklist writers produce separate acceptance checklists.
2. Axon merges the proposals deterministically. An item is `REQUIRED` only when both writers independently include the same normalized key and mark it required. Every other proposed item is `OPTIONAL`.
3. Two workers receive the merged checklist and bounded task context. They execute independently and return artifacts/evidence.
4. Axon runs deterministic checks against the resulting artifacts (tests, builds, formatting, schema and policy checks). Model assertions are not deterministic checks.
5. Two independent supervisors review the worker outputs and check evidence.
6. Axon accepts only when both supervisors approve with evidence and every deterministic check passes.
7. A failed check or a request for changes causes a repair round. At most two repair rounds are allowed. A run that still fails after the second repair is marked failed; there is no unbounded retry loop.

The Rust implementation in `backend/axum/src/supervisor.rs` provides the deterministic checklist merge, two-worker evidence validation, final review decision functions, and a guarded run state machine. Workspace-scoped Axum routes persist run snapshots through the existing SQLite entity store and write corresponding audit events. The core is intentionally provider-neutral and has no direct model API dependency.

## Deterministic checklist merge

Checklist keys are trimmed and normalized to lowercase ASCII. Valid keys contain only letters, digits, dots, underscores and hyphens. Duplicate keys from one writer are rejected rather than silently collapsed. Acceptance criteria are unioned, sorted and deduplicated. Output ordering is lexical, so swapping writer order does not change the merged result.

The merge rule is:

| Writer A | Writer B | Merged priority |
| --- | --- | --- |
| required | required | REQUIRED |
| required | optional | OPTIONAL |
| required | absent | OPTIONAL |
| optional | required | OPTIONAL |
| optional | optional | OPTIONAL |
| absent | required | OPTIONAL |

## Evidence and final decision

Each supervisor review must come from a distinct, non-empty supervisor identity and cite at least one known deterministic check with a non-empty source and observation. Unknown check references and evidence-free reviews are rejected.

The decision function returns:

- `accepted`: both reviews approve and all deterministic checks pass.
- `repair_required`: a check fails or either supervisor requests changes, while fewer than two repair rounds have been used.
- `failed`: the same conditions remain after two repair rounds.

A review cannot turn a failed deterministic check into a pass. An empty review set, a single review, duplicate supervisor identities, invalid check identifiers, or a repair count above two is rejected.

## Security boundaries

- Checklist proposals, worker outputs and supervisor reviews are untrusted model/provider input.
- Axon owns run identity, workspace authorization, state transitions, retry count and final decision.
- Provider credentials are never placed in task context or returned to the browser.
- Evidence records must point to artifacts or deterministic check results that Axon can resolve; a free-form model assertion is not proof.
- Worker and supervisor roles must use independent provider calls or independently configured agents. Reusing one response under two labels does not satisfy independence.
- No worker or supervisor receives broad workspace access by default. Context is scoped to the task and explicitly selected artifacts.

## API surface

All endpoints require a Firebase ID token and workspace membership. The API derives the human actor from the verified token; caller-supplied writer, worker and supervisor identifiers are workflow provenance labels, not authentication credentials.

- `POST /api/v1/workspaces/{workspace_id}/supervisor-runs` — create a run with a title and objective.
- `GET /api/v1/workspaces/{workspace_id}/supervisor-runs` — list runs in the workspace.
- `GET /api/v1/workspaces/{workspace_id}/supervisor-runs/{run_id}` — retrieve a run.
- `POST .../{run_id}/checklists` — submit one writer's checklist proposal.
- `POST .../{run_id}/workers` — submit one worker's result and artifact references.
- `POST .../{run_id}/checks` — record deterministic check outcomes and evidence URIs.
- `POST .../{run_id}/reviews` — submit an evidence-bearing supervisor review.
- `POST .../{run_id}/repair` — begin a bounded repair round after a repair-required decision.

Run state is stored as a typed entity in SQLite and each state transition is accompanied by an audit event. In-process transitions are serialized to prevent concurrent requests from overwriting one another. The current deployment remains single-instance; multi-instance optimistic concurrency is a production gate.

## Current implementation boundary

The deterministic core, guarded state machine, workspace-scoped API, SQLite persistence and unit tests are present. Provider calls, automatic parallel worker dispatch, registered-agent credential binding, CI/build/test runner integration, repair-task generation, and the web/mobile supervisor dashboard are not yet complete. The API currently accepts role-labelled submissions from an authenticated workspace user; distinct labels alone do not prove that separate model instances performed the work. Do not claim full autonomous supervision until those execution and independence controls are wired and tested.
