# Repository-wide agent instructions

These rules are mandatory for every automated or human-assisted engineering task in this repository.

## Single-branch workflow: `master` only

- **All implementation, fixes, tests, documentation, configuration, and integration changes MUST be committed directly to `master`.**
- Before reading or changing code, verify that the target ref is `master`. Fetch/read files from `master`, and pass `master` explicitly to every repository write.
- **Do not create, use, or switch to feature, task, phase, staging, development, hotfix, or other working branches.**
- **Do not open pull requests or require PR merges** for routine repository work. Make the requested changes directly on `master`.
- Do not leave completed work stranded on another branch. If existing work is found elsewhere, inspect it and consolidate the intended, verified changes onto `master` before continuing.
- Never force-push, reset, or overwrite unrelated work. If `master` has moved, re-read its latest state and apply changes without discarding commits.
- If a requested operation cannot be performed safely on `master`, stop that operation and explain the exact blocker; do not silently create another branch as a workaround.

## Required execution discipline

1. Inspect the current `master` state before editing.
2. Preserve existing architecture, APIs, data, and user decisions unless the task explicitly authorizes a change.
3. Implement complete behavior; do not substitute TODOs, stubs, fake success paths, or documentation-only claims for required code.
4. Add or update focused tests and documentation with each relevant implementation change.
5. Run the applicable format, lint, build, and test commands. Report commands that could not be run and why; never claim unexecuted checks passed.
6. Commit coherent completed changes directly to `master` with descriptive messages.
7. Continue through the remaining requested work instead of stopping after an audit or producing a backlog as a substitute for implementation.
8. Do not deploy, change production infrastructure, rotate credentials, or perform destructive data operations unless explicitly authorized.

## Reporting

At completion, report the exact `master` commit SHA, the files and behavior changed, checks actually executed and their results, and any remaining verified blockers. Be explicit about anything not tested or not completed.
