use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use chrono::{Duration, NaiveDate, NaiveDateTime};
use clap::{Parser, Subcommand};
use rand::RngCore;
use serde::{Deserialize, Serialize};

#[derive(Parser)]
#[command(about = "Manage project milestone events in Google Calendar")]
struct Args {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Create all milestone events from a plan file
    Add {
        plan: PathBuf,
    },
    /// List all calendar events for a project ID
    List {
        project_id: String,
        #[arg(short, long)]
        calendar: Option<String>,
    },
    /// Delete all calendar events for a project ID
    Remove {
        project_id: String,
        #[arg(short, long)]
        calendar: Option<String>,
    },
    /// Remove existing events and re-add from plan file
    Sync {
        plan: PathBuf,
    },
    /// Scaffold a new plan file with a generated project ID
    Init {
        /// Output file path (prints to stdout if omitted)
        output: Option<PathBuf>,
    },
}

#[derive(Deserialize, Serialize)]
struct Plan {
    project: Project,
    milestones: Vec<Milestone>,
}

#[derive(Deserialize, Serialize)]
struct Project {
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    name: String,
    url: String,
    calendar: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    timezone: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct Milestone {
    title: String,
    date: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    time: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_mins: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    attendees: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    timezone: Option<String>,
}

struct CalEvent {
    date: String,
    title: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    match args.cmd {
        Cmd::Add { plan } => cmd_add(&plan),
        Cmd::List { project_id, calendar } => cmd_list(&project_id, calendar.as_deref()),
        Cmd::Remove { project_id, calendar } => cmd_remove(&project_id, calendar.as_deref()),
        Cmd::Sync { plan } => cmd_sync(&plan),
        Cmd::Init { output } => cmd_init(output.as_deref()),
    }
}

fn generate_id() -> String {
    let mut bytes = [0u8; 4];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

fn load_plan(path: &Path) -> Result<Plan, Box<dyn std::error::Error>> {
    let content = fs::read_to_string(path)?;
    Ok(toml::from_str(&content)?)
}

fn ensure_id(plan: &mut Plan, path: &Path) -> Result<String, Box<dyn std::error::Error>> {
    if plan.project.id.is_none() {
        let id = generate_id();
        plan.project.id = Some(id.clone());
        let updated = toml::to_string_pretty(plan)?;
        fs::write(path, updated)?;
        eprintln!("Generated project ID: {} (saved to {})", id, path.display());
    }
    Ok(plan.project.id.clone().unwrap())
}

fn build_ics(plan: &Plan, project_id: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut ics = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//gcal-plan//EN\r\n");

    for (i, m) in plan.milestones.iter().enumerate() {
        let mut uid_bytes = [0u8; 4];
        rand::thread_rng().fill_bytes(&mut uid_bytes);
        let uid_rand: String = uid_bytes.iter().map(|b| format!("{:02x}", b)).collect();
        let uid = format!("{}-{}-{}@gcal-plan", project_id, i, uid_rand);

        let (dtstart, dtend) = if let Some(time) = &m.time {
            let dt_str = format!("{} {}", m.date, time);
            let start = NaiveDateTime::parse_from_str(&dt_str, "%Y-%m-%d %H:%M")?;
            let mins = m.duration_mins.unwrap_or(60) as i64;
            let end = start + Duration::minutes(mins);
            let s = start.format("%Y%m%dT%H%M%S");
            let e = end.format("%Y%m%dT%H%M%S");
            let tz_raw = m.timezone.as_deref().or(plan.project.timezone.as_deref());
            match tz_raw.map(parse_tz).as_deref() {
                Some("Z") => (format!("DTSTART:{s}Z"), format!("DTEND:{e}Z")),
                Some(off) if off.starts_with('+') || off.starts_with('-') => (
                    format!("DTSTART:{s}{off}"),
                    format!("DTEND:{e}{off}"),
                ),
                Some(iana) => (
                    format!("DTSTART;TZID={iana}:{s}"),
                    format!("DTEND;TZID={iana}:{e}"),
                ),
                None => (format!("DTSTART:{s}"), format!("DTEND:{e}")),
            }
        } else {
            if m.duration_mins.is_some() {
                eprintln!("Warning: '{}': duration_mins ignored (no time — all-day event)", m.title);
            }
            if m.timezone.is_some() {
                eprintln!("Warning: '{}': timezone ignored (no time — all-day event)", m.title);
            }
            let start = NaiveDate::parse_from_str(&m.date, "%Y-%m-%d")?;
            let end = start + Duration::days(1);
            (
                format!("DTSTART;VALUE=DATE:{}", start.format("%Y%m%d")),
                format!("DTEND;VALUE=DATE:{}", end.format("%Y%m%d")),
            )
        };

        let meta = format!(
            "project-id: {}\nproject-name: {}\nproject-url: {}",
            project_id, plan.project.name, plan.project.url
        );
        let desc = match &m.description {
            Some(d) => format!("{}\n\n{}", d, meta),
            None => meta,
        };

        ics.push_str("BEGIN:VEVENT\r\n");
        ics.push_str(&format!("UID:{}\r\n", uid));
        ics.push_str(&format!("SUMMARY:{}\r\n", ics_escape(&m.title)));
        ics.push_str(&format!("{}\r\n", dtstart));
        ics.push_str(&format!("{}\r\n", dtend));
        ics.push_str(&format!("DESCRIPTION:{}\r\n", ics_escape(&desc)));
        ics.push_str(&format!("URL:{}\r\n", plan.project.url));
        for attendee in m.attendees.iter().flatten() {
            ics.push_str(&format!("{}\r\n", ics_attendee(attendee)));
        }
        ics.push_str("END:VEVENT\r\n");
    }

    ics.push_str("END:VCALENDAR\r\n");
    Ok(ics)
}

fn parse_tz(s: &str) -> String {
    let s = s.trim();
    if s.eq_ignore_ascii_case("utc") {
        return "Z".to_string();
    }
    if let Some(rest) = s.strip_prefix(['+', '-']) {
        let sign = &s[..1];
        let digits: String = rest.chars().filter(|c| c.is_ascii_digit()).collect();
        let normalized = match digits.len() {
            2 => format!("{digits}00"), // "-06" → "0600"
            _ => digits,               // "-0600", "-06:00" (colon already stripped)
        };
        return format!("{sign}{normalized}");
    }
    s.to_string()
}

fn ics_attendee(s: &str) -> String {
    // Accept "Name <email>" or bare "email"
    if let Some(rest) = s.strip_suffix('>') {
        if let Some((name, email)) = rest.split_once('<') {
            let name = name.trim();
            let email = email.trim();
            return format!("ATTENDEE;CN={}:mailto:{}", name, email);
        }
    }
    format!("ATTENDEE:mailto:{}", s.trim())
}

fn ics_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
}

fn import_plan(plan: &Plan, project_id: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let ics = build_ics(plan, project_id)?;
    let tmp_path = std::env::temp_dir().join(format!("gcal-plan-{}.ics", project_id));
    fs::write(&tmp_path, &ics)?;

    let mut cmd = Command::new("gcalcli");
    cmd.arg("import");
    cmd.args(["--calendar", &plan.project.calendar]);
    cmd.arg(&tmp_path);
    let output = cmd.output()?;
    fs::remove_file(&tmp_path).ok();

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    // Forward gcalcli's output so the user sees warnings/errors
    if !stdout.is_empty() { print!("{}", stdout); }
    if !stderr.is_empty() { eprint!("{}", stderr); }

    if !output.status.success() {
        return Err("gcalcli import failed".into());
    }
    Ok(parse_added_count(&format!("{}{}", stdout, stderr)))
}

fn parse_added_count(output: &str) -> usize {
    output.lines()
        .find_map(|line| {
            let n_str = line.strip_prefix("Added ")?.split_whitespace().next()?;
            n_str.parse().ok()
        })
        .unwrap_or(0)
}

fn search_events(project_id: &str, calendar: Option<&str>) -> Result<Vec<CalEvent>, Box<dyn std::error::Error>> {
    let query = format!("project-id: {}", project_id);
    let mut cmd = Command::new("gcalcli");
    cmd.args(["search", &query, "--details", "all", "--tsv", "--nocolor", "--military"]);
    if let Some(cal) = calendar {
        cmd.args(["--calendar", cal]);
    }

    let output = cmd.output()?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("gcalcli search failed: {}", stderr).into());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut lines = stdout.lines();
    let Some(header) = lines.next() else {
        return Ok(vec![]);
    };
    let headers: Vec<&str> = header.split('\t').collect();

    let query_lower = query.to_lowercase();
    let mut events = Vec::new();

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

        events.push(CalEvent {
            date: get("start_date").to_string(),
            title: get("title").to_string(),
        });
    }

    events.sort_by(|a, b| a.date.cmp(&b.date));
    Ok(events)
}

fn cmd_add(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut plan = load_plan(path)?;
    let project_id = ensure_id(&mut plan, path)?;
    let added = import_plan(&plan, &project_id)?;
    println!(
        "Added {}/{} milestone(s) for project {} ({})",
        added,
        plan.milestones.len(),
        project_id,
        plan.project.name
    );
    Ok(())
}

fn resolve_project(arg: &str) -> Result<(String, Option<String>), Box<dyn std::error::Error>> {
    let path = Path::new(arg);
    if path.exists() {
        let plan = load_plan(path)?;
        let id = plan.project.id.ok_or("plan file has no project id — run `gcal-plan add` first")?;
        Ok((id, Some(plan.project.calendar)))
    } else {
        Ok((arg.to_string(), None))
    }
}

fn cmd_list(project_id: &str, calendar: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let (project_id, plan_calendar) = resolve_project(project_id)?;
    let calendar = calendar.or(plan_calendar.as_deref());
    let events = search_events(&project_id, calendar)?;
    if events.is_empty() {
        println!("No events found for project-id: {}", project_id);
        return Ok(());
    }
    println!("Events for project {}:", project_id);
    for e in &events {
        println!("  {}  {}", e.date, e.title);
    }
    Ok(())
}

fn cmd_remove(project_id: &str, calendar: Option<&str>) -> Result<(), Box<dyn std::error::Error>> {
    let (project_id, plan_calendar) = resolve_project(project_id)?;
    let calendar = calendar.or(plan_calendar.as_deref());
    let events = search_events(&project_id, calendar)?;
    if events.is_empty() {
        println!("No events found for project-id: {}", project_id);
        return Ok(());
    }

    let query = format!("project-id: {}", project_id);
    let dates: HashSet<String> = events.iter().map(|e| e.date.clone()).collect();
    let mut removed = 0;

    for date in &dates {
        let d = NaiveDate::parse_from_str(date, "%Y-%m-%d")?;
        let d_next = d + Duration::days(1);
        let start = d.format("%Y-%m-%d").to_string();
        let end = d_next.format("%Y-%m-%d").to_string();

        let mut cmd = Command::new("gcalcli");
        cmd.args(["delete", "--iamaexpert", &query, &start, &end]);
        if let Some(cal) = calendar {
            cmd.args(["--calendar", cal]);
        }
        if cmd.status()?.success() {
            removed += 1;
        } else {
            eprintln!("Warning: delete failed for {}", date);
        }
    }

    println!("Removed events on {} date(s) for project {}", removed, project_id);
    Ok(())
}

fn cmd_sync(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut plan = load_plan(path)?;
    let project_id = ensure_id(&mut plan, path)?;
    cmd_remove(&project_id, Some(&plan.project.calendar))?;
    let added = import_plan(&plan, &project_id)?;

    println!(
        "Synced {}/{} milestone(s) for project {} ({})",
        added,
        plan.milestones.len(),
        project_id,
        plan.project.name
    );
    Ok(())
}

fn cmd_init(output: Option<&Path>) -> Result<(), Box<dyn std::error::Error>> {
    let id = generate_id();
    let skeleton = format!(
        r#"[project]
id = "{id}"
name = "My Project"
url = "https://github.com/org/repo/issues/1"
calendar = "Calendar Name"
# timezone = "US/Central"     # default for all timed milestones; also accepts -0600, -06:00, -06, UTC

[[milestones]]
title = "Kickoff"
date = "2026-06-01"
# description = "Optional description"

[[milestones]]
title = "Design Complete"
date = "2026-07-01"
# time = "14:00"       # makes this a timed event
# duration_mins = 60   # default: 60 when time is set
# timezone = "US/Central"  # overrides project-level timezone for this milestone
# attendees = ["user@example.com", "Name <user@example.com>"]
"#
    );

    match output {
        Some(path) => {
            fs::write(path, &skeleton)?;
            println!("Created {}", path.display());
        }
        None => print!("{}", skeleton),
    }
    Ok(())
}
