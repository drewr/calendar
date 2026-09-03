//! Generate a gantt-style project schedule sheet in an Excel workbook.

use chrono::{Datelike, Duration, NaiveDate};
use clap::Parser;
use rust_xlsxwriter::{Format, FormatAlign, FormatBorder, Workbook};

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

#[derive(Parser)]
#[command(
    about = "Create a new Excel file with a gantt-style project schedule.",
    after_help = "Example: gen-project-schedule 2026-09-01 12"
)]
struct Args {
    /// Start date (YYYY-MM-DD). Snapped to the preceding Monday.
    start_date: String,

    /// Number of weeks
    #[arg(default_value_t = 14)]
    weeks: u32,

    /// Output file path (default: '<tab-name> Schedule <start-date>.xlsx')
    #[arg(short, long)]
    output: Option<String>,

    /// Number of blank rows for people's names
    #[arg(short, long, default_value_t = 15)]
    people: u32,
}

fn parse_date(s: &str) -> NaiveDate {
    match NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        Ok(d) => d,
        Err(_) => {
            eprintln!("Error: invalid date '{s}'. Use YYYY-MM-DD format.");
            std::process::exit(1);
        }
    }
}

/// Return d if it's a Monday, else the preceding Monday.
fn prev_monday(d: NaiveDate) -> NaiveDate {
    let days_since_monday = d.weekday().num_days_from_monday();
    d - Duration::days(days_since_monday as i64)
}

/// Return "D Mon", e.g. "31 Aug" or "7 Sep".
fn week_label(d: NaiveDate) -> String {
    format!("{} {}", d.day(), MONTHS[d.month0() as usize])
}

/// Months with 14+ days in the range (trims fringe months at either end).
fn core_months(start: NaiveDate, weeks: u32) -> Vec<NaiveDate> {
    let end = start + Duration::weeks(weeks as i64) - Duration::days(1);
    let mut result = Vec::new();
    let mut cur = start.with_day(1).unwrap();
    while cur <= end {
        let next_month = if cur.month() == 12 {
            NaiveDate::from_ymd_opt(cur.year() + 1, 1, 1).unwrap()
        } else {
            NaiveDate::from_ymd_opt(cur.year(), cur.month() + 1, 1).unwrap()
        };
        let range_end = std::cmp::min(next_month - Duration::days(1), end);
        let range_start = std::cmp::max(cur, start);
        let days_in_range = (range_end - range_start).num_days() + 1;
        if days_in_range >= 14 {
            result.push(cur);
        }
        cur = next_month;
    }
    if result.is_empty() {
        result.push(start.with_day(1).unwrap());
    }
    result
}

fn sheet_title(start: NaiveDate, weeks: u32) -> String {
    let months = core_months(start, weeks);
    let (first, last) = (months[0], *months.last().unwrap());
    if months.len() == 1 {
        format!("{}", first.format("%B %Y"))
    } else if first.year() == last.year() {
        format!("{} - {} {}", first.format("%B"), last.format("%B"), first.year())
    } else {
        format!("{} - {}", first.format("%B %Y"), last.format("%B %Y"))
    }
}

fn tab_name(start: NaiveDate, weeks: u32) -> String {
    let months = core_months(start, weeks);
    let (first, last) = (months[0], *months.last().unwrap());
    if months.len() == 1 {
        format!("{}", first.format("%b %Y"))
    } else if first.year() == last.year() {
        format!("{}-{} {}", first.format("%b"), last.format("%b"), first.year())
    } else {
        format!("{} - {}", first.format("%b %Y"), last.format("%b %Y"))
    }
}

fn build_sheet(
    workbook: &mut Workbook,
    start: NaiveDate,
    weeks: u32,
    people: u32,
) -> Result<(), rust_xlsxwriter::XlsxError> {
    let name = tab_name(start, weeks);
    let sheet = workbook.add_worksheet();
    sheet.set_name(&name)?;

    let last_col = weeks as u16; // column A (index 0) is names; B.. are weeks

    let title_format = Format::new()
        .set_bold()
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter);
    let header_format = Format::new()
        .set_bold()
        .set_align(FormatAlign::Center)
        .set_border_bottom(FormatBorder::Thin);
    let name_format = Format::new()
        .set_align(FormatAlign::Left)
        .set_align(FormatAlign::Top)
        .set_border_top(FormatBorder::Thin)
        .set_border_bottom(FormatBorder::Thin)
        .set_border_left(FormatBorder::Thin)
        .set_border_right(FormatBorder::Thin);
    let cell_format = Format::new()
        .set_align(FormatAlign::Left)
        .set_align(FormatAlign::Top)
        .set_text_wrap()
        .set_border_top(FormatBorder::Thin)
        .set_border_bottom(FormatBorder::Thin)
        .set_border_left(FormatBorder::Thin)
        .set_border_right(FormatBorder::Thin);

    // Row 1: merged title
    sheet.merge_range(0, 0, 0, last_col, &sheet_title(start, weeks), &title_format)?;
    sheet.set_row_height(0, 30.75)?;

    // Row 2: headers — "Name" then one column per week
    sheet.write_string_with_format(1, 0, "Name", &header_format)?;
    for week in 0..weeks {
        let d = start + Duration::weeks(week as i64);
        sheet.write_string_with_format(1, 1 + week as u16, week_label(d), &header_format)?;
    }
    sheet.set_row_height(1, 15)?;

    // set_column_width() bakes in extra cell padding that Excel's own
    // pixels-to-chars formula doesn't remove when writing the width back out,
    // so it overshoots the requested character width. Pass pixels directly
    // (chars * default digit width) to land close to the intended value.
    sheet.set_column_width_pixels(0, (18.0_f64 * 7.0).round() as u32)?;
    for col in 1..=last_col {
        sheet.set_column_width_pixels(col, (16.0_f64 * 7.0).round() as u32)?;
    }

    // Blank rows for people's names and their weekly assignments
    for r in 0..people {
        let row = 2 + r;
        sheet.set_row_height(row, 30.0)?;
        sheet.write_blank(row, 0, &name_format)?;
        for col in 1..=last_col {
            sheet.write_blank(row, col, &cell_format)?;
        }
    }

    Ok(())
}

fn main() {
    let args = Args::parse();

    let start = prev_monday(parse_date(&args.start_date));

    let mut workbook = Workbook::new();
    build_sheet(&mut workbook, start, args.weeks, args.people).unwrap_or_else(|e| {
        eprintln!("Error building sheet: {e}");
        std::process::exit(1);
    });

    let tab = tab_name(start, args.weeks);
    let out = args
        .output
        .clone()
        .unwrap_or_else(|| format!("{tab} Schedule {start}.xlsx"));

    workbook.save(&out).unwrap_or_else(|e| {
        eprintln!("Error saving {out}: {e}");
        std::process::exit(1);
    });

    let end = start + Duration::weeks(args.weeks as i64) - Duration::days(1);
    println!("Created {out} — '{tab}' ({})", sheet_title(start, args.weeks));
    println!(
        "  {start} – {end} ({} weeks, {} people rows)",
        args.weeks, args.people
    );
}
