# Contributing

`breadmon` is deprecated. It is no longer shipped with BOS or listed in the
bakery registry. The repository remains available for historical reference and
small maintenance fixes. No new dev, beta, or stable releases are planned.

## Branches

There is one long-lived branch, `main`. Use a short-lived `feature/<name>` or
`fix/<name>` branch for changes, then open a pull request into `main`. Delete
the branch after merge.

## Local development

```sh
cargo build --release
cargo test --release
```

Cargo.toml pins `bread-utils` to a release tag. To test a change against the
latest `bread-ecosystem` main branch, use the shared dev dependency script:

```sh
/path/to/bread-ecosystem/scripts/dev-deps.sh --root .
cargo build --release
```

The script writes a generated, gitignored `.cargo/config.toml` and updates
Cargo.lock. Discard those local changes after testing.

## CI

The `check.yml` workflow runs dependency pin linting, clippy, and tests for
`feature/**` and `fix/**` branches. It resolves shared dependencies against
their main branches. Release workflows and bakery packaging metadata have
been removed.

## Questions

Open an issue on this repository's Forgejo tracker.
