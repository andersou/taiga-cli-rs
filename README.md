# taiga-cli-rs

Rust CLI and reusable async client for the [Taiga REST API](https://docs.taiga.io/api.html). It supports Taiga.io and self-hosted instances, defaults to human-readable output, and provides JSON output for automation.

## Features

- Normal Taiga username/email and password authentication, token refresh, and private local token storage.
- Projects, memberships, roles, statistics, user stories, tasks, issues, epics, milestones, wiki, search, notifications, and timelines.
- Optimistic-concurrency updates using Taiga resource versions.
- Project-scoped and all-visible-project full-text search.
- Precompiled binaries published as GitHub release archives for Linux x86_64, Windows x86_64, Intel macOS, and Apple Silicon macOS.

## Install

Download a precompiled archive from the [GitHub releases](https://github.com/andersou/taiga-cli-rs/releases) page and place the `taiga-cli` binary on your `PATH`. Each release ships archives for the four supported targets plus a `SHA256SUMS` file for verification.

To build from source instead, see the sections below.

### Agent skill

From a local checkout, install the global skill for supported coding agents:

```sh
npx skills add . --global --skill taiga-cli --yes
```


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

The binary is written to `target/release/taiga-cli`.

## Authentication

Log in interactively:

```sh
taiga-cli auth login --username you@example.com
```

For scripts, pass credentials through the environment and read the password from standard input:

```sh
printf '%s' "$TAIGA_PASSWORD" | taiga-cli auth login --username "$TAIGA_USERNAME" --password-stdin
```

Set `TAIGA_API_URL` to a self-hosted host, `/api`, or `/api/v1` URL. The client normalizes it to `/api/v1/`.

```sh
TAIGA_API_URL=https://taiga.example.com taiga-cli auth login --username you@example.com
```

Tokens and the API URL are saved to the platform configuration directory, or to `TAIGA_CONFIG` when that variable is set. `TAIGA_AUTH_TOKEN` and `TAIGA_REFRESH_TOKEN` override stored credentials for a single process and are never persisted.

### Session renewal

Taiga issues short-lived tokens (on taiga.io the access token lasts one hour and the refresh token three days). The CLI keeps the session alive without extra round trips:

1. The access token is used while its JWT `exp` claim is still valid; an expired one is skipped instead of producing a failed request.
2. The refresh token rotates the pair, and the new tokens are saved.
3. When both tokens are gone, the CLI logs in again with, in order: `TAIGA_PASSWORD` from the environment, the password saved with `--remember`, or an interactive prompt when stdin is a terminal. Otherwise it fails with `no valid session, run \`taiga-cli auth login\``.

Re-login with stored or prompted credentials only happens for the server saved in the configuration; with `--api-url` or `TAIGA_API_URL` pointing elsewhere, or with `TAIGA_AUTH_TOKEN` set, stored credentials are never sent.

To skip the prompt entirely, save the password in the system keychain (macOS Keychain, Windows Credential Manager, or the Secret Service on Linux):

```sh
taiga-cli auth login --username you@example.com --remember
taiga-cli auth forget    # drop the saved password, keep the session
taiga-cli auth logout    # drop tokens and the saved password
```

Once a password is saved, later `taiga-cli auth login` runs for the same account keep it up to date until `forget` or `logout`. Keep in mind that a saved password grants access for as long as it stays valid, unlike a refresh token that expires on its own. Headless Linux hosts without a Secret Service daemon cannot store passwords; use `TAIGA_PASSWORD` there instead.

## Commands

```sh
taiga-cli --help
taiga-cli project list
taiga-cli userstory list --project 123
taiga-cli milestone list --project 123 --closed false
taiga-cli userstory list --project 123 --milestone 17
taiga-cli task status 42 7
taiga-cli issue create --project 123 --subject "Broken API response"
taiga-cli epic stories list 9
taiga-cli milestone stats 17
taiga-cli wiki get 42
taiga-cli search "authentication" --project 123
taiga-cli notification list --unread
taiga-cli timeline project 123
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
