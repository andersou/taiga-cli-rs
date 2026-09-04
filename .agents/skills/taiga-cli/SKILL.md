---
name: taiga-cli
description: Use the taiga-cli command to read and manage Taiga (taiga.io or self-hosted) projects, user stories, tasks, issues, epics, milestones, wiki, search, notifications, and timelines from the terminal. Use when asked to inspect or change anything in a Taiga project.
---

# taiga-cli

Rust CLI for the Taiga REST API. Output is pretty-printed JSON; pass `--output json` for guaranteed machine-readable stdout (errors always go to stderr). Commands take Taiga numeric database IDs, not the `#ref` numbers shown in the Taiga UI — translate with `get-by-ref`.

## Binary

If `taiga-cli` is not on PATH, do not build anything on your own — tell the user to download a precompiled archive from the GitHub releases page (https://github.com/andersou/taiga-cli-rs/releases) for their platform (Linux x86_64, Windows x86_64, Intel macOS, or Apple Silicon macOS) and place the extracted `taiga-cli` binary on PATH.

Only if the user prefers building from source, any Rust 1.98.0+ toolchain works — rustup, vfox (the repo ships a `.vfox.toml`), or another manager:

```sh
cargo build --release --package taiga-cli
```

Binary: `target/release/taiga-cli`.

If `taiga-cli` is on PATH but older than the latest release, `taiga-cli update --check` says so and `taiga-cli update` replaces the binary with the verified release archive; no login is needed.

## Server and authentication

- Default server `https://api.taiga.io/api/v1`. Self-hosted: `--api-url` or `TAIGA_API_URL` (host, `/api`, or `/api/v1` forms all accepted).
- Log in once; tokens persist with mode 0600 in the platform config dir (macOS `~/Library/Application Support/taiga-cli/config.json`, Linux `~/.config/taiga-cli/config.json`) or in `TAIGA_CONFIG` when that variable is set.

```sh
taiga-cli auth login --username you@example.com     # prompts for password
taiga-cli auth login --username you@example.com --remember   # also saves the password in the OS keychain
printf '%s' "$TAIGA_PASSWORD" | taiga-cli auth login --username "$TAIGA_USERNAME" --password-stdin
taiga-cli auth status                               # verify identity, api_url, password_stored
taiga-cli auth forget                               # remove the saved password, keep the session
taiga-cli auth logout                               # remove tokens and the saved password
```

- `login` also reads `TAIGA_USERNAME` and `TAIGA_PASSWORD` from the environment.
- `TAIGA_AUTH_TOKEN` overrides the stored token for a single process and is never persisted.
- Sessions renew automatically: the access token is refreshed when its JWT `exp` passes, and once the refresh token (3 days on taiga.io) is gone the CLI logs in again using `TAIGA_PASSWORD`, the keychain password saved by `--remember`, or a terminal prompt. Without any of those it exits 5 with `no valid session, run \`taiga-cli auth login\``; in non-interactive runs set `TAIGA_PASSWORD` or ask the user to run `taiga-cli auth login --remember` once.
- Stored credentials are only used for the server in the config file; with `--api-url` or `TAIGA_API_URL` pointing elsewhere, log in explicitly.

## Exit codes

`0` success · `2` usage error · `3` config error · `5` API/other error · `6` optimistic-concurrency conflict (re-run the edit; the version is re-read automatically) · `7` Taiga rate limit (wait, then retry).

## Find IDs first

```sh
taiga-cli project list                       # only projects you are a member of
taiga-cli project list --all-visible         # every visible project
taiga-cli userstory statuses --project 123   # status IDs; also: task/issue/epic statuses
taiga-cli issue metadata --project 123       # issue statuses, types, priorities, severities
taiga-cli project roles --project 123
taiga-cli project memberships --project 123  # user IDs for --assigned-to
```

## Generic resource verbs

`project`, `userstory`, `task`, `issue`, `epic`, `milestone`, and `wiki` share the same verbs:

```sh
taiga-cli userstory list --project 123 --status 4 --assigned-to 12 --closed false --tag api --tag bug
taiga-cli userstory list --project 123 --milestone 17    # stories in one sprint; also for task/issue
taiga-cli milestone list --project 123 --closed false    # open sprints only
taiga-cli userstory get 4567
taiga-cli userstory get-by-ref 42 --project 123     # UI "#42" -> full object including id
taiga-cli issue create --project 123 --subject "Broken API response" --description "..." --status 4
taiga-cli issue edit 4567 --status 5                # needs at least one field
taiga-cli issue delete 4567 --yes                   # refuses without --yes
taiga-cli userstory history 4567 --kind comment     # comments only; --kind activity for field changes
taiga-cli userstory attachments 4567
taiga-cli project stats 123                         # also: taiga milestone stats 17
```

- Lists return page 1 by default; use `--page N` for more. JSON output is `{"items": [...], "pagination": {...}}` and `pagination.count` is the total.
- `edit` fetches the current resource `version` and sends it back, so concurrent edits fail with exit code 6 instead of silently overwriting; just re-run.
- `--data '<json object>'` on `create`/`edit` merges arbitrary Taiga API fields, e.g. `--data '{"assigned_to":12,"milestone":17}'`. Explicit flags override `--data` keys.
- Wiki pages have no subject; create them through `--data`: `taiga wiki create --data '{"project":123,"slug":"home","content":"..."}'`.

## Resource-specific commands

```sh
taiga-cli task status 4567 8                     # move task 4567 to status 8
taiga-cli epic stories list 9
taiga-cli epic stories add 9 4567
taiga-cli epic stories reorder 9 4567 --order 2
taiga-cli epic stories remove 9 4567 --yes
```

## Current sprint and current stories

When asked about the current sprint, "what is being worked on", or a story that is in progress, start from the open sprints instead of paging through every milestone or every story:

```sh
taiga-cli milestone list --project 123 --closed false --output json | jq '.items[] | {id, name, estimated_start, estimated_finish}'
taiga-cli userstory list --project 123 --milestone 17 --output json | jq '.items[] | {id, ref, subject, status_extra_info}'
```

- Usually one sprint is open; when several are, pick the one whose `estimated_start`/`estimated_finish` covers today, then confirm with the user.
- Stories with no sprint are in the backlog: `taiga-cli userstory list --project 123 --closed false` and pick items with `"milestone": null`.
- Only fall back to `taiga-cli milestone list --project 123` without `--closed false` (all sprints, paginated) when the user asks for history or a sprint that is already closed.
- `taiga-cli milestone stats 17` summarizes points and completed stories for a sprint.

## Search

```sh
taiga-cli search "authentication" --project 123
taiga-cli search "authentication" --project-slug my-project
taiga-cli search "authentication" --all-projects   # iterates every visible project; slow
```

## Notifications and timelines

```sh
taiga-cli notification list --unread
taiga-cli notification count
taiga-cli notification read 555
taiga-cli notification read-all
taiga-cli timeline project 123 --relevant
taiga-cli timeline user 12                         # also: taiga timeline profile 12
```

## Automation

```sh
taiga-cli userstory list --project 123 --output json | jq '.items[] | {id, ref, subject}'
taiga-cli userstory get-by-ref 42 --project 123 --output json | jq .id
```
