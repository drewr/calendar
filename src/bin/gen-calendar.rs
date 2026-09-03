//! Generate a 14-week calendar sheet in an Excel workbook.

use chrono::{Datelike, Duration, NaiveDate};
use clap::Parser;
use rust_xlsxwriter::{Color, Format, FormatAlign, FormatBorder, Workbook};

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const GRAY_FILL: Color = Color::RGB(0xF0F0F0);

#[derive(Parser)]
#[command(
    about = "Create a new Excel file with a 14-week calendar sheet.",
    after_help = "Example: gen-calendar 2026-04-05"
)]
struct Args {
    /// Start date (YYYY-MM-DD). Snapped to the preceding Sunday.
    start_date: String,

    /// Output file path (default: '<tab-name>.xlsx', e.g. 'Apr-Jun 2026.xlsx')
    #[arg(short, long)]
    output: Option<String>,

    /// Number of weeks
    #[arg(short, long, default_value_t = 14)]
    weeks: u32,
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

/// Return d if it's a Sunday, else the preceding Sunday.
fn prev_sunday(d: NaiveDate) -> NaiveDate {
    let days_since_sunday = d.weekday().num_days_from_sunday();
    d - Duration::days(days_since_sunday as i64)
}

/// Return "Mon D" at a month boundary, else just "D".
fn cell_label(d: NaiveDate, prev: Option<NaiveDate>) -> String {
    match prev {
        Some(p) if p.month() == d.month() => d.day().to_string(),
        _ => format!("{} {}", MONTHS[d.month0() as usize], d.day()),
    }
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
) -> Result<(), rust_xlsxwriter::XlsxError> {
    let name = tab_name(start, weeks);
    let sheet = workbook.add_worksheet();
    sheet.set_name(&name)?;

    let title_format = Format::new()
        .set_bold()
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter);
    let header_format = Format::new().set_bold().set_align(FormatAlign::Center);
    let week_num_format = Format::new()
        .set_align(FormatAlign::Center)
        .set_align(FormatAlign::VerticalCenter);

    let date_format = Format::new()
        .set_align(FormatAlign::Left)
        .set_align(FormatAlign::Top)
        .set_border_top(FormatBorder::Thin)
        .set_border_left(FormatBorder::Thin)
        .set_border_right(FormatBorder::Thin);
    let date_format_gray = date_format.clone().set_background_color(GRAY_FILL);

    let bottom_format = Format::new()
        .set_border_bottom(FormatBorder::Thin)
        .set_border_left(FormatBorder::Thin)
        .set_border_right(FormatBorder::Thin);
    let bottom_format_gray = bottom_format.clone().set_background_color(GRAY_FILL);

    // Row 1: merged title
    sheet.merge_range(0, 0, 0, 7, &sheet_title(start, weeks), &title_format)?;
    sheet.set_row_height(0, 30.75)?;

    // Row 2: day headers
    for (col, h) in ["Notes", "Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"]
        .iter()
        .enumerate()
    {
        sheet.write_string_with_format(1, col as u16, *h, &header_format)?;
    }
    sheet.set_row_height(1, 15)?;

    // set_column_width() bakes in extra cell padding that Excel's own
    // pixels-to-chars formula doesn't remove when writing the width back out,
    // so it overshoots the requested character width. Pass pixels directly
    // (chars * default digit width) to land close to the intended value.
    sheet.set_column_width_pixels(0, (7.13_f64 * 7.0).round() as u32)?;
    sheet.set_column_width_pixels(1, (8.38_f64 * 7.0).round() as u32)?;

    let all_days: Vec<NaiveDate> = (0..weeks * 7)
        .map(|i| start + Duration::days(i as i64))
        .collect();

    for week in 0..weeks {
        let date_row = 2 + week * 2;
        let empty_row = date_row + 1;
        sheet.set_row_height(date_row, 17.25)?;
        sheet.set_row_height(empty_row, 17.25)?;
        sheet.merge_range(date_row, 0, empty_row, 0, "", &week_num_format)?;

        for day_offset in 0..7u32 {
            let idx = (week * 7 + day_offset) as usize;
            let d = all_days[idx];
            let prev = if idx > 0 { Some(all_days[idx - 1]) } else { None };
            let col = (1 + day_offset) as u16; // column B (index 1) = Sunday
            let is_weekend = day_offset == 0 || day_offset == 6;

            let date_fmt = if is_weekend { &date_format_gray } else { &date_format };
            sheet.write_string_with_format(date_row, col, cell_label(d, prev), date_fmt)?;

            let bottom_fmt = if is_weekend { &bottom_format_gray } else { &bottom_format };
            sheet.write_blank(empty_row, col, bottom_fmt)?;
        }
    }

    Ok(())
}

fn main() {
    let args = Args::parse();

    let start = prev_sunday(parse_date(&args.start_date));

    let mut workbook = Workbook::new();
    build_sheet(&mut workbook, start, args.weeks).unwrap_or_else(|e| {
        eprintln!("Error building sheet: {e}");
        std::process::exit(1);
    });

    let tab = tab_name(start, args.weeks);
    let out = args
        .output
        .clone()
        .unwrap_or_else(|| format!("{tab} {start}.xlsx"));

    workbook.save(&out).unwrap_or_else(|e| {
        eprintln!("Error saving {out}: {e}");
        std::process::exit(1);
    });

    let end = start + Duration::weeks(args.weeks as i64) - Duration::days(1);
    println!("Created {out} — '{tab}' ({})", sheet_title(start, args.weeks));
    println!("  {start} – {end} ({} weeks)", args.weeks);
}
