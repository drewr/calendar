---
name: gen-calendar
description: Generate a new 14-week calendar Excel file for a given date range
argument-hint: <start-date YYYY-MM-DD>
---

Extract the start date from $ARGUMENTS, then run:

```bash
python3 gen_calendar.py <start-date>
```

The date is snapped to the preceding Sunday automatically. If no date is given, ask the user for one.

**Options:**
- `--output <path>` — custom output filename (default: `<tab-name>.xlsx`, e.g. `Apr-Jun 2026.xlsx`)
- `--weeks <N>` — number of weeks (default: 14)

Confirm success with the output line, e.g.:
```
Created Apr-Jun 2026.xlsx — 'Apr-Jun 2026' (April - June 2026)
  2026-04-05 – 2026-07-11 (14 weeks)
```
