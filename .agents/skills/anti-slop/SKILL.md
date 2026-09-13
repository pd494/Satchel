---
name: anti-slop
description: Run and maintain Satchel's project-local anti-slop Oxlint rules. Use for lint cleanup, rule verification, or reviewed upstream updates.
---

# Anti-slop

This directory is both the Codex skill and the canonical Oxlint plugin. Keep a
single copy here; `oxlint.config.ts` loads `index.ts` and `effect/index.ts`
directly.

## Cleanup

1. Read `AGENTS.md` and inspect `git status` before changing owned source.
2. Run `bun run lint:oxlint` to exercise every enabled generic and Effect rule.
3. When cleanup is authorized, use `bun run lint:oxlint --fix`, then
   `bun run format`, `bun run lint`, `bun run check`, and `bun run test`.
4. Do not weaken severities, suppress findings, or add unsafe casts to pass.
5. Confirm a second fix/format pass is stable.

## Updates

Treat these rules as vendored project policy. Stage upstream changes outside
this directory, compare behavior and dependencies, preserve local decisions,
and update `UPSTREAM.md` with the exact source revision and deviations. Never
restore a second runtime copy under `tools/`.

CI validates every `.agents/skills/*/SKILL.md` and runs Oxlint through the
quality job.
