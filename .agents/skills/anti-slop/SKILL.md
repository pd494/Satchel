---
name: anti-slop
description: Run and maintain Satchel's project-local anti-slop Oxlint rules. Use for lint cleanup, rule verification, or reviewed upstream updates.
---

# Anti-slop

This directory contains only project guidance and provenance. The executable
plugin is installed from the immutable Git revision recorded in `UPSTREAM.md`;
the lint script compiles its entry points into ignored cache files before
`oxlint.config.ts` loads them.

## Cleanup

1. Read `AGENTS.md` and inspect `git status` before changing owned source.
2. Run `bun run lint:oxlint` to exercise every enabled generic and Effect rule.
3. When cleanup is authorized, use `bun run lint:oxlint --fix`, then
   `bun run format`, `bun run lint`, `bun run check`, and `bun run test`.
4. Do not weaken severities, suppress findings, or add unsafe casts to pass.
5. Confirm a second fix/format pass is stable.

## Updates

Treat these rules as pinned project policy. Review upstream changes before
updating the Git revision, keep Oxlint aligned with the version tested upstream,
and update `UPSTREAM.md`. Do not vendor the implementation into this repository.

CI validates every `.agents/skills/*/SKILL.md` and runs Oxlint through the
quality job.
