# AGENTS.md

## Build, Lint, and Test Commands
- Build all: `cargo build --all-targets --all-features`
- Lint: `cargo fmt --all -- --check` and `cargo clippy --all-targets --all-features -- -D warnings`
- Check: `cargo check --all-targets --all-features`
- Test all: `cargo test --all-targets --all-features`
- Run a single test: `cargo test <test_name>` or `cargo test --test <file_name>`
- Run with output: `cargo test -- --nocapture`

## Code Style Guidelines
- Use `rustfmt` for formatting; always run `cargo fmt` before submitting.
- Imports: Group std, external, and internal separately; use explicit paths.
- Types: Prefer explicit types, strong typing, and enums for variants.
- Naming: Snake_case for variables/functions, CamelCase for types/structs/traits.
- Error handling: Use `Result`/`Option`, propagate errors with `?`, avoid panics in production.
- Tests: Place unit tests in `src/`, integration in `tests/`, use `test_` prefix.
- All code and tests must pass CI (see `.github/workflows/ci.yml`).

## GitHub/PM Workflow
- Use `dev` as the base branch for features/PRs.
- See `.roo/rules-orchestrator/gh.md` for required `gh` CLI commands for issues/PRs/project management.

## Misc
- Document new strategies and pool models in crate-level README files.
- Follow performance and error handling targets in test README.
