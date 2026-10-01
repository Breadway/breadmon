# AGENTS.md: Repo hygiene

This repository is deprecated. Follow `CONTRIBUTING.md` for any maintenance
work. Use short-lived `feature/x` or `fix/x` branches off the single `main`
branch. Do not cut new releases; bakery metadata and release workflows have
been removed.

## Remotes
- `origin`: Forgejo (`git.breadway.dev`, SSH), authoritative.
- `github`: GitHub mirror. Push `origin` only; the mirror syncs automatically.

## CI
- `check.yml`: clippy + test, triggers on push to `feature/**`/`fix/**`.
The check runs on a self-hosted runner inside a pinned Arch
container, not the host's native environment. The Containerfile/build
script are shared across bread-ecosystem products and live in
`bread-ecosystem/ci/`; this repo's `ci/build.sh` clones that repo at the
sha in `ci/bread-ecosystem.rev` (deliberately pinned, not `main`) and
delegates to it.

## Don't
- Don't embed credentials in remote URLs; SSH or a credential helper only.

## Persistent memory
Read `~/.knowledge.md` at the start of a task; it points to a shared memory index at `~/.claude/memory/MEMORY.md` (also used by Claude Code). Selectively read the linked entries relevant to what you're working on.
