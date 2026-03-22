# Skill: /gen-calendar

Add a new 14-week calendar sheet to `Calendar Templates.xlsx` (or any Excel file).

## Usage

```
/gen-calendar <start-date> [options]
```

**Arguments:**
- `<start-date>` — ISO date (`YYYY-MM-DD`) of the first day of the period. Automatically snapped to the preceding Sunday.
- `--file <path>` — Target Excel file (default: `Calendar Templates.xlsx`)
- `--weeks <N>` — Number of weeks to generate (default: 14)

## Examples

```bash
# Add Apr–Jun 2026 quarter (starts week of Apr 5)
nix run .#gen-calendar -- 2026-04-05

# Custom range: 14 weeks starting week of Jul 6
nix run .#gen-calendar -- 2026-07-06

# Different file
nix run .#gen-calendar -- 2026-10-04 --file MyCalendar.xlsx

# Without nix (if openpyxl is available)
python3 gen_calendar.py 2026-04-05
```

## What it does

1. Parses the start date and snaps it back to Sunday if needed
2. Opens the target Excel file (creates it if missing)
3. Adds a new sheet named like `Apr-Jun 2026` with:
   - Row 1: Merged title spanning all 8 columns (e.g. "April - June 2026")
   - Row 2: Headers — Notes, Sun, Mon, Tue, Wed, Thu, Fri, Sat
   - Rows 3–30: 14 weeks, 2 rows each (date labels + blank note row)
   - Column A (Notes) merged across both rows per week
   - Month-change boundary labels: "Apr 1" on first day of a new month
   - Light gray fill on Sundays and Saturdays
4. Saves the file in place

## How Claude should invoke this skill

When the user runs `/gen-calendar`, extract the start date from their message and run:

```bash
nix run .#gen-calendar -- <start-date>
```

If `openpyxl` is available in the current environment, you may run directly:

```bash
python3 gen_calendar.py <start-date>
```

After running, confirm the sheet was added by checking the output line like:
```
Added sheet 'Apr-Jun 2026' (April - June 2026) to Calendar Templates.xlsx
```
