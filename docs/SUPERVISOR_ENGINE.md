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

The first implementation in `backend/axum/src/supervisor.rs` provides the deterministic checklist merge and final review decision functions. It is intentionally provider-neutral and has no direct model API dependency.

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

## Current implementation boundary

The deterministic core and unit tests are present. This commit does not yet claim that provider calls, parallel worker execution, durable run persistence, repair-task creation, or the web/mobile supervisor dashboard are complete. Those behaviors must be wired through authenticated workspace-scoped APIs and persisted state transitions before the engine is considered end-to-end.
