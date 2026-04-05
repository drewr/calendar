use std::process::Command;

use clap::{Parser, ValueEnum};

#[derive(Clone, ValueEnum)]
enum Format {
    Tsv,
    Csv,
}

#[derive(Parser)]
#[command(about = "Search Google Calendar events via gcalcli")]
struct Args {
    /// Search string to match against all event fields
    query: String,

    /// Calendar name (defaults to primary account calendar)
    #[arg(short, long)]
    calendar: Option<String>,

    /// Output format
    #[arg(short, long, default_value = "tsv")]
    format: Format,
}

struct Event {
    id: String,
    datetime: String,
    title: String,
    description: String,
}

fn main() {
    let args = Args::parse();

    let mut cmd = Command::new("gcalcli");
    cmd.args([
        "search",
        &args.query,
        "--details",
        "all",
        "--tsv",
        "--nocolor",
        "--military",
    ]);
    if let Some(cal) = &args.calendar {
        cmd.args(["--calendar", cal]);
    }

    let output = cmd.output().expect("failed to run gcalcli");
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("gcalcli error: {stderr}");
        std::process::exit(1);
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();

    let Some(header) = lines.next() else { return };
    let headers: Vec<&str> = header.split('\t').collect();

    let query_lower = args.query.to_lowercase();
    let mut events: Vec<Event> = Vec::new();

    for line in lines {
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < headers.len() {
            continue;
        }

        let get = |name: &str| -> &str {
            headers
                .iter()
                .position(|h| *h == name)
                .and_then(|i| fields.get(i).copied())
                .unwrap_or("")
        };

        if !fields.iter().any(|f| f.to_lowercase().contains(&query_lower)) {
            continue;
        }

        let datetime = format!("{} {}", get("start_date"), get("start_time"));
        events.push(Event {
            id: get("id").to_string(),
            datetime,
            title: get("title").to_string(),
            description: get("description").to_string(),
        });
    }

    events.sort_by(|a, b| a.datetime.cmp(&b.datetime));

    let sep = match args.format {
        Format::Tsv => '\t',
        Format::Csv => ',',
    };

    for e in &events {
        let desc_short: String = e.description.chars().take(50).collect();
        match args.format {
            Format::Tsv => {
                println!("{}{sep}{}{sep}{}{sep}{}", e.id, e.datetime, e.title, desc_short);
            }
            Format::Csv => {
                println!(
                    "{},{},{},{}",
                    csv_quote(&e.id),
                    csv_quote(&e.datetime),
                    csv_quote(&e.title),
                    csv_quote(&desc_short),
                );
            }
        }
    }
}

fn csv_quote(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
