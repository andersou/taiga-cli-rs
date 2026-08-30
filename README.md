# taiga-cli-rs

Rust CLI and reusable async client for the [Taiga REST API](https://docs.taiga.io/api.html). It supports Taiga.io and self-hosted instances, defaults to human-readable output, and provides JSON output for automation.

## Features

- Normal Taiga username/email and password authentication, token refresh, and private local token storage.
- Projects, memberships, roles, statistics, user stories, tasks, issues, epics, milestones, wiki, search, notifications, and timelines.
- Optimistic-concurrency updates using Taiga resource versions.
- Project-scoped and all-visible-project full-text search.
- Precompiled binaries published as GitHub release archives for Linux x86_64, Windows x86_64, Intel macOS, and Apple Silicon macOS.

## Install

Download a precompiled archive from the [GitHub releases](https://github.com/andersou/taiga-cli-rs/releases) page and place the `taiga` binary on your `PATH`. Each release ships archives for the four supported targets plus a `SHA256SUMS` file for verification.

To build from source instead, see the sections below.

## Requirements

- Rust 1.98.0 or newer. Install it however you prefer: [rustup](https://rustup.rs/) or any other toolchain manager works. If you use [vfox](https://vfox.lhan.me/), the repository ships a `.vfox.toml` and the command below selects the pinned toolchain — but vfox is only a suggestion, not a requirement.
- A Taiga.io or self-hosted Taiga API instance.

```sh
# optional, only if you use vfox:
vfox use -p rust@1.98.0
```

## Build and test

```sh
cargo test --workspace --all-targets --all-features --locked
cargo build --release --package taiga-cli
```

The binary is written to `target/release/taiga`.

## Authentication

Log in interactively:

```sh
taiga auth login --username you@example.com
```

For scripts, pass credentials through the environment and read the password from standard input:

```sh
printf '%s' "$TAIGA_PASSWORD" | taiga auth login --username "$TAIGA_USERNAME" --password-stdin
```

Set `TAIGA_API_URL` to a self-hosted host, `/api`, or `/api/v1` URL. The client normalizes it to `/api/v1/`.

```sh
TAIGA_API_URL=https://taiga.example.com taiga auth login --username you@example.com
```

Tokens and the API URL are saved to the platform configuration directory, or to `TAIGA_CONFIG` when that variable is set. `TAIGA_AUTH_TOKEN` and `TAIGA_REFRESH_TOKEN` override stored credentials for a single process and are never persisted.

## Commands

```sh
taiga --help
taiga project list
taiga userstory list --project 123
taiga task status 42 7
taiga issue create --project 123 --subject "Broken API response"
taiga epic stories list 9
taiga milestone stats 17
taiga wiki get 42
taiga search "authentication" --project 123
taiga notification list --unread
taiga timeline project 123
```

Every command accepts `--output human` (default) or `--output json`. JSON success output is written to stdout; errors are written to stderr with a nonzero exit status.

## Releases

Conventional Commits determine releases:

- `feat`: minor release.
- `fix`, `perf`, `revert`: patch release.
- `!` or a `BREAKING CHANGE` footer: major release.
- `docs`, `chore`, `ci`, `test`, `refactor`, `style`, and `build`: no release unless breaking.

GitHub Actions tests the workspace and creates native archives for the four supported targets. Pushes to `main` create stable releases; pushes to `develop` create `beta` prereleases. Each release contains archives and `SHA256SUMS`.

Run the release planner locally after installing Node dependencies:

```sh
npm ci --ignore-scripts
npm run release:dry-run -- --no-ci
```

Publishing is CI-only and requires `EXPECTED_VERSION`.

## Quality hooks

[prek](https://prek.j178.dev/) runs repository checks, workflow linting, Conventional Commit validation, formatting, Clippy, and pre-push tests.

```sh
prek install
prek run --all-files
prek run cargo-test --hook-stage pre-push --all-files
```

## License

MIT. See [LICENSE](LICENSE).
