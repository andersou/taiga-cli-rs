---
name: taiga-cli
description: Use the taiga CLI to read and manage Taiga (taiga.io or self-hosted) projects, user stories, tasks, issues, epics, milestones, wiki, search, notifications, and timelines from the terminal. Use when asked to inspect or change anything in a Taiga project.
---

# taiga CLI

Rust CLI for the Taiga REST API. Output is pretty-printed JSON; pass `--output json` for guaranteed machine-readable stdout (errors always go to stderr). Commands take Taiga numeric database IDs, not the `#ref` numbers shown in the Taiga UI — translate with `get-by-ref`.

## Binary

If `taiga` is not on PATH, do not build anything on your own — tell the user to download a precompiled archive from the GitHub releases page (https://github.com/andersou/taiga-cli-rs/releases) for their platform (Linux x86_64, Windows x86_64, Intel macOS, or Apple Silicon macOS) and place the extracted `taiga` binary on PATH.

Only if the user prefers building from source, any Rust 1.98.0+ toolchain works — rustup, vfox (the repo ships a `.vfox.toml`), or another manager:

```sh
cargo build --release --package taiga-cli
```

Binary: `target/release/taiga`.

## Server and authentication

- Default server `https://api.taiga.io/api/v1`. Self-hosted: `--api-url` or `TAIGA_API_URL` (host, `/api`, or `/api/v1` forms all accepted).
- Log in once; tokens persist with mode 0600 in the platform config dir (macOS `~/Library/Application Support/taiga-cli/config.json`, Linux `~/.config/taiga-cli/config.json`) or in `TAIGA_CONFIG` when that variable is set.

```sh
taiga auth login --username you@example.com     # prompts for password
printf '%s' "$TAIGA_PASSWORD" | taiga auth login --username "$TAIGA_USERNAME" --password-stdin
taiga auth status                               # verify identity and api_url
taiga auth logout
```

- `login` also reads `TAIGA_USERNAME` and `TAIGA_PASSWORD` from the environment.
- `TAIGA_AUTH_TOKEN` overrides the stored token for a single process and is never persisted.
- The CLI does not auto-refresh expired tokens: on authentication errors, run `taiga auth login` again.

## Exit codes

`0` success · `2` usage error · `3` config error · `5` API/other error · `6` optimistic-concurrency conflict (re-run the edit; the version is re-read automatically) · `7` Taiga rate limit (wait, then retry).

## Find IDs first

```sh
taiga project list                       # only projects you are a member of
taiga project list --all-visible         # every visible project
taiga userstory statuses --project 123   # status IDs; also: task/issue/epic statuses
taiga issue metadata --project 123       # issue statuses, types, priorities, severities
taiga project roles --project 123
taiga project memberships --project 123  # user IDs for --assigned-to
```

## Generic resource verbs

`project`, `userstory`, `task`, `issue`, `epic`, `milestone`, and `wiki` share the same verbs:

```sh
taiga userstory list --project 123 --status 4 --assigned-to 12 --closed false --tag api --tag bug
taiga userstory get 4567
taiga userstory get-by-ref 42 --project 123     # UI "#42" -> full object including id
taiga issue create --project 123 --subject "Broken API response" --description "..." --status 4
taiga issue edit 4567 --status 5                # needs at least one field
taiga issue delete 4567 --yes                   # refuses without --yes
taiga userstory history 4567 --kind comment     # comments only; --kind activity for field changes
taiga userstory attachments 4567
taiga project stats 123                         # also: taiga milestone stats 17
```

- Lists return page 1 by default; use `--page N` for more. JSON output is `{"items": [...], "pagination": {...}}` and `pagination.count` is the total.
- `edit` fetches the current resource `version` and sends it back, so concurrent edits fail with exit code 6 instead of silently overwriting; just re-run.
- `--data '<json object>'` on `create`/`edit` merges arbitrary Taiga API fields, e.g. `--data '{"assigned_to":12,"milestone":17}'`. Explicit flags override `--data` keys.
- Wiki pages have no subject; create them through `--data`: `taiga wiki create --data '{"project":123,"slug":"home","content":"..."}'`.

## Resource-specific commands

```sh
taiga task status 4567 8                     # move task 4567 to status 8
taiga epic stories list 9
taiga epic stories add 9 4567
taiga epic stories reorder 9 4567 --order 2
taiga epic stories remove 9 4567 --yes
```

## Search

```sh
taiga search "authentication" --project 123
taiga search "authentication" --project-slug my-project
taiga search "authentication" --all-projects   # iterates every visible project; slow
```

## Notifications and timelines

```sh
taiga notification list --unread
taiga notification count
taiga notification read 555
taiga notification read-all
taiga timeline project 123 --relevant
taiga timeline user 12                         # also: taiga timeline profile 12
```

## Automation

```sh
taiga userstory list --project 123 --output json | jq '.items[] | {id, ref, subject}'
taiga userstory get-by-ref 42 --project 123 --output json | jq .id
```
