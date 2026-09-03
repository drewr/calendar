# calendar

Tools for managing calendars from the command line.

## Requirements

`gen-calendar` has no external dependencies. `gcal-plan` and `gcal-search` require [gcalcli](https://github.com/insanum/gcalcli) to be authenticated first — run `gcalcli list` to verify your account is connected.

---

## gen-calendar

Create a new Excel file with a 14-week calendar sheet.

```
gen-calendar <start-date> [--output <path>] [--weeks <N>]
```

`start-date` (YYYY-MM-DD) is snapped to the preceding Sunday. Output defaults to `<tab-name> <start-date>.xlsx` (e.g. `Apr-Jun 2026 2026-04-05.xlsx`); `--weeks` defaults to 14.

### Running via Nix

```
nix run .#gen-calendar -- 2026-04-05
```

---

## gcal-plan

Import a project plan into Google Calendar. Each milestone becomes a calendar event tagged with a unique project ID, so you can list, update, or remove the whole set later.

### Plan file format

Create a TOML file describing your project:

```toml
[project]
id = "deadbeef"        # auto-generated on first add if omitted
name = "Q2 Migration"
url = "https://github.com/org/repo/issues/42"
calendar = "Datum Engineering"
timezone = "US/Central"         # default for all timed milestones

[[milestones]]
title = "Kickoff"
date = "2026-06-01"
description = "Initial planning session"   # optional

[[milestones]]
title = "Design Review"
date = "2026-06-15"
time = "14:00"          # omit for an all-day event
duration_mins = 90      # minutes; default 60 when time is set
timezone = "US/Eastern" # overrides project timezone for this milestone
attendees = [
  "jsmith@datum.net",
  "Jane Doe <jdoe@datum.net>",   # Name <email> also accepted
]
```

`gcal-plan init` scaffolds a starter file with a fresh project ID.

`timezone` applies only to timed milestones (those with a `time` field); all-day events ignore it. Accepted formats: IANA names (`US/Central`, `America/New_York`), UTC offsets (`-0600`, `-06:00`, `-06`), or `UTC`. Case-insensitive for `UTC` and offset formats.

### Commands

```
gcal-plan init [output-file]       Scaffold a new plan file (stdout if no file given)
gcal-plan add <plan.toml>          Create all milestone events in Google Calendar
gcal-plan list <project-id>        List calendar events for a project
gcal-plan remove <project-id>      Delete all calendar events for a project
gcal-plan sync <plan.toml>         Remove existing events and re-add from plan (update)
```

`list` and `remove` accept `--calendar <name>` to scope to a specific calendar.

### How project IDs work

When `id` is absent from the plan file, `gcal-plan add` generates an 8-character hex ID and writes it back into the file. Every event's description contains:

```
project-id: deadbeef
project-name: Q2 Migration
project-url: https://github.com/org/repo/issues/42
```

This makes events findable via `gcal-search` or `gcalcli search "project-id: deadbeef"`.

### Running via Nix

```
nix run .#gcal-plan -- add plan.toml
nix run .#gcal-plan -- list deadbeef
```

---

## gcal-search

Search Google Calendar events and print results as TSV or CSV.

```
gcal-search <query> [--calendar <name>] [--format tsv|csv]
```

Output columns: event ID, datetime, title, description (truncated to 50 chars).

Results are sorted chronologically and filtered to rows where at least one field matches the query.

### Running via Nix

```
nix run . -- "project-id: deadbeef"
nix run .#gcal-search -- "standup" --calendar "Datum Engineering" --format csv
```

---

## Development

```
cargo build          # build all binaries
cargo run --bin gcal-plan -- init
cargo run --bin gcal-search -- "my query"
cargo run --bin gen-calendar -- 2026-04-05
```
