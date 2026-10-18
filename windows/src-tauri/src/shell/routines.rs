// Routines (docs/WINDOWS_SHELL.md): « OK Coucou », tous les lundis à 9 h, ouvre
// Unity et Spotify. When one is created, Claude turns the request into a fixed
// list of steps — level 1 tools with their parameters, nothing else — and the
// user approves that list on a card (shell/confirm.rs). When it is due, Coucou
// replays the list as it is, without calling any model: a web page can never
// change what a routine does. Kept in the config folder as JSON, listed and
// deletable in Settings; they run only while Coucou runs and the panel is on.
//
// The scheduler is one thread that sleeps until the next routine is due (at
// most 15 minutes at a time, so a changed clock or a sleeping computer is
// caught up with), and not at all while there is no routine.

use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter};

pub const MAX_ROUTINES: usize = 20;
pub const MAX_STEPS: usize = 8;
const MAX_NAME_CHARS: usize = 60;
const MAX_INPUT_BYTES: usize = 1024;

/// What a step may do: the level 1 tools, and opening an app. Never a
/// reading tool (nobody would hear it), never a level 2 one.
pub const STEP_TOOLS: &[&str] = &[
    "open_app",
    "set_volume",
    "toggle_wifi",
    "toggle_bluetooth",
    "focus_window",
    "snap_window",
    "apply_layout",
    "set_wallpaper",
    "web_open",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum When {
    /// On these days (1 Monday … 7 Sunday) at `time` ("09:00").
    Weekly { days: Vec<u8>, time: String },
    /// Once, on `date` ("2026-11-10") at `time`.
    Once { date: String, time: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub tool: String,
    pub input: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Routine {
    pub id: u64,
    pub name: String,
    pub when: When,
    pub steps: Vec<Step>,
    /// The minute it last ran (minutes since 1970-01-01, local time).
    #[serde(default)]
    pub last_run: Option<i64>,
}

// ── Time, in local minutes ────────────────────────────────────────────────────

/// Days since 1970-01-01 for a civil date (Howard Hinnant's algorithm).
pub fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// 1 Monday … 7 Sunday, for a day count since 1970-01-01 (a Thursday).
pub fn weekday(days: i64) -> u8 {
    ((days + 3).rem_euclid(7) + 1) as u8
}

fn parse_time(t: &str) -> Option<i64> {
    let (h, m) = t.split_once(':')?;
    let (h, m): (i64, i64) = (h.parse().ok()?, m.parse().ok()?);
    ((0..24).contains(&h) && (0..60).contains(&m)).then_some(h * 60 + m)
}

fn parse_date(d: &str) -> Option<i64> {
    let mut it = d.split('-');
    let (y, m, day): (i64, i64, i64) = (it.next()?.parse().ok()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
    if it.next().is_some() || !(2000..=2100).contains(&y) || !(1..=12).contains(&m) || !(1..=31).contains(&day) {
        return None;
    }
    // 31 February would roll over into March: refuse it.
    let first = days_from_civil(y, m, 1);
    let next = if m == 12 { days_from_civil(y + 1, 1, 1) } else { days_from_civil(y, m + 1, 1) };
    (day <= next - first).then_some(first + day - 1)
}

pub fn now_minutes() -> i64 {
    let t = crate::platform::local_time();
    days_from_civil(t.year as i64, t.month as i64, t.day as i64) * 1440 + t.hour as i64 * 60 + t.minute as i64
}

/// The first time `when` is due at or after minute `from`; None once a one-off is past.
pub fn next_due(when: &When, from: i64) -> Option<i64> {
    match when {
        When::Weekly { days, time } => {
            let t = parse_time(time)?;
            let today = from.div_euclid(1440);
            (0..=7).map(|d| today + d).find_map(|day| {
                let at = day * 1440 + t;
                (days.contains(&weekday(day)) && at >= from).then_some(at)
            })
        }
        When::Once { date, time } => {
            let at = parse_date(date)? * 1440 + parse_time(time)?;
            (at >= from).then_some(at)
        }
    }
}

// ── Checking what Claude proposed ─────────────────────────────────────────────

/// A routine as it may be kept: a known schedule, a few allowed steps, bounded inputs.
pub fn validate(name: &str, when: When, steps: Vec<Step>) -> Result<Routine, String> {
    let bad = |why: &str| Err(format!("Not a routine Coucou can keep: {why}."));
    let name: String = super::today::clean_line(name, MAX_NAME_CHARS);
    if name.is_empty() {
        return bad("it needs a short name");
    }
    match &when {
        When::Weekly { days, time } => {
            if days.is_empty() || days.iter().any(|d| !(1..=7).contains(d)) || parse_time(time).is_none() {
                return bad("the days are 1 (Monday) to 7 (Sunday) and the time is HH:MM");
            }
        }
        When::Once { date, time } => {
            if parse_date(date).is_none() || parse_time(time).is_none() {
                return bad("the date is YYYY-MM-DD and the time HH:MM");
            }
            if next_due(&when, now_minutes()).is_none() {
                return bad("that time is already past");
            }
        }
    }
    if steps.is_empty() || steps.len() > MAX_STEPS {
        return bad("it needs between 1 and 8 steps");
    }
    for s in &steps {
        if !STEP_TOOLS.contains(&s.tool.as_str()) {
            return bad(&format!("{} cannot be a step (only {})", s.tool, STEP_TOOLS.join(", ")));
        }
        if !s.input.is_object() || serde_json::to_vec(&s.input).map_or(true, |b| b.len() > MAX_INPUT_BYTES) {
            return bad("each step's input is a small object");
        }
    }
    let mut days_sorted = when;
    if let When::Weekly { days, .. } = &mut days_sorted {
        days.sort_unstable();
        days.dedup();
    }
    Ok(Routine { id: 0, name, when: days_sorted, steps, last_run: None })
}

/// A step as the card and Settings show it: "web_open · amazon: papier cadeau".
pub fn step_summary(s: &Step) -> String {
    let words: Vec<String> = s
        .input
        .as_object()
        .map(|o| {
            o.values()
                .filter_map(|v| match v {
                    Value::String(x) => Some(x.clone()),
                    Value::Bool(b) => Some(if *b { "on".into() } else { "off".into() }),
                    Value::Number(n) => Some(n.to_string()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let detail: String = words.join(": ").chars().take(80).collect();
    if detail.is_empty() {
        s.tool.clone()
    } else {
        format!("{} · {detail}", s.tool)
    }
}

// ── Storage ───────────────────────────────────────────────────────────────────

fn path() -> std::path::PathBuf {
    crate::settings::config_dir().join("shell-routines.json")
}

static LIST: Mutex<Option<Vec<Routine>>> = Mutex::new(None);
static WAKE: Condvar = Condvar::new();

fn loaded(slot: &mut Option<Vec<Routine>>) -> &mut Vec<Routine> {
    slot.get_or_insert_with(|| {
        std::fs::read(path())
            .ok()
            .and_then(|b| serde_json::from_slice::<Vec<Routine>>(&b).ok())
            .unwrap_or_default()
            .into_iter()
            .filter_map(validate_stored)
            .take(MAX_ROUTINES)
            .collect()
    })
}

fn with_list<R>(f: impl FnOnce(&mut Vec<Routine>) -> R) -> R {
    let mut guard = LIST.lock().unwrap_or_else(|e| e.into_inner());
    f(loaded(&mut guard))
}

/// A routine read from disk keeps only what validate would have let through.
fn validate_stored(r: Routine) -> Option<Routine> {
    let ok = !r.steps.is_empty()
        && r.steps.len() <= MAX_STEPS
        && r.steps.iter().all(|s| STEP_TOOLS.contains(&s.tool.as_str()) && s.input.is_object())
        && match &r.when {
            When::Weekly { days, time } => !days.is_empty() && days.iter().all(|d| (1..=7).contains(d)) && parse_time(time).is_some(),
            When::Once { date, time } => parse_date(date).is_some() && parse_time(time).is_some(),
        };
    ok.then_some(r)
}

fn save(list: &[Routine]) -> Result<(), String> {
    let p = path();
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

pub fn list() -> Vec<Routine> {
    with_list(|l| l.clone())
}

/// Keeps a routine the user approved on its card.
pub fn add(mut r: Routine) -> Result<(), String> {
    with_list(|l| {
        if l.len() >= MAX_ROUTINES {
            return Err(crate::i18n::t("You already have 20 routines: delete one in Settings first."));
        }
        r.id = l.iter().map(|x| x.id).max().map_or(1, |m| m + 1);
        l.push(r);
        save(l)
    })?;
    WAKE.notify_all();
    Ok(())
}

pub fn delete(id: u64) -> Result<Vec<Routine>, String> {
    let out = with_list(|l| {
        l.retain(|r| r.id != id);
        save(l).map(|_| l.clone())
    });
    WAKE.notify_all();
    out
}

// ── The scheduler ─────────────────────────────────────────────────────────────

/// The routines due at minute `now` that have not run for that occurrence.
/// A routine is due for two minutes, so waking a little late still runs it.
pub fn due(list: &[Routine], now: i64) -> Vec<(u64, i64)> {
    list.iter()
        .filter_map(|r| {
            let at = next_due(&r.when, now - 2)?;
            (at <= now && r.last_run != Some(at)).then_some((r.id, at))
        })
        .collect()
}

/// Starts the scheduler once; it sleeps while there is nothing to do.
pub fn start(app: &AppHandle) {
    static STARTED: OnceLock<()> = OnceLock::new();
    if STARTED.set(()).is_err() {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || loop {
        let now = now_minutes();
        let ran = run_due(&app, now);
        if ran {
            continue;
        }
        // Looked at under the lock the wait releases, so an add in between is never missed.
        let mut guard = LIST.lock().unwrap_or_else(|e| e.into_inner());
        let next = loaded(&mut guard).iter().filter_map(|r| next_due(&r.when, now + 1)).min();
        match next {
            // Nothing scheduled: sleep until a routine is added.
            None => drop(WAKE.wait(guard)),
            Some(at) => {
                let second = crate::platform::local_time().second as i64;
                let secs = ((at - now) * 60 - second).clamp(1, 15 * 60) as u64;
                drop(WAKE.wait_timeout(guard, Duration::from_secs(secs)));
            }
        }
    });
}

fn panel_on(app: &AppHandle) -> bool {
    use tauri::Manager;
    app.try_state::<crate::Shared>().is_some_and(|s| s.settings.lock().unwrap().shell_panel)
}

/// Runs what is due now; true if anything ran.
fn run_due(app: &AppHandle, now: i64) -> bool {
    let todo = with_list(|l| due(l, now));
    if todo.is_empty() {
        return false;
    }
    for (id, at) in todo {
        // Marked first, so a step that hangs can never make it run twice.
        let routine = with_list(|l| {
            let r = l.iter_mut().find(|r| r.id == id)?;
            r.last_run = Some(at);
            let copy = r.clone();
            if matches!(r.when, When::Once { .. }) {
                l.retain(|r| r.id != id);
            }
            let _ = save(l);
            Some(copy)
        });
        let Some(routine) = routine else { continue };
        if !panel_on(app) {
            continue;
        }
        let mut failed = 0;
        for step in &routine.steps {
            let (_, did) = crate::voice::run_tool(app, &step.tool, &step.input);
            if !did {
                failed += 1;
            }
        }
        let message = if failed == 0 {
            crate::i18n::tf("Routine “{name}” ran.", &[("name", &routine.name)])
        } else {
            crate::i18n::tf("Routine “{name}” ran; {n} step(s) did nothing.", &[("name", &routine.name), ("n", &failed.to_string())])
        };
        let _ = app.emit_to(crate::island::WINDOW_LABEL, "shell-routine-ran", message);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn step(tool: &str) -> Step {
        Step { tool: tool.into(), input: json!({ "name": "Spotify" }) }
    }

    #[test]
    fn civil_days_and_weekdays() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(weekday(0), 4, "1970-01-01 was a Thursday");
        let d = days_from_civil(2026, 10, 18);
        assert_eq!(weekday(d), 7, "18 October 2026 is a Sunday");
        assert_eq!(days_from_civil(2024, 3, 1) - days_from_civil(2024, 2, 28), 2, "2024 is a leap year");
        assert!(parse_date("2026-02-31").is_none());
        assert!(parse_date("2026-13-01").is_none());
        assert!(parse_date("26-01-01").is_none());
        assert_eq!(parse_time("09:05"), Some(545));
        assert!(parse_time("24:00").is_none());
    }

    #[test]
    fn the_next_occurrence_of_a_weekly_or_one_off_routine() {
        let sunday = days_from_civil(2026, 10, 18) * 1440;
        let mondays = When::Weekly { days: vec![1], time: "09:00".into() };
        assert_eq!(next_due(&mondays, sunday + 600), Some(sunday + 1440 + 540));
        // At 09:00 on a Monday it is due now; a minute later, next week.
        let monday9 = sunday + 1440 + 540;
        assert_eq!(next_due(&mondays, monday9), Some(monday9));
        assert_eq!(next_due(&mondays, monday9 + 1), Some(monday9 + 7 * 1440));
        let once = When::Once { date: "2026-11-10".into(), time: "11:00".into() };
        let at = days_from_civil(2026, 11, 10) * 1440 + 660;
        assert_eq!(next_due(&once, sunday), Some(at));
        assert_eq!(next_due(&once, at + 1), None);
    }

    #[test]
    fn a_routine_is_due_once_per_occurrence_even_woken_late() {
        let monday9 = days_from_civil(2026, 10, 19) * 1440 + 540;
        let mut r = Routine { id: 1, name: "x".into(), when: When::Weekly { days: vec![1], time: "09:00".into() }, steps: vec![step("open_app")], last_run: None };
        assert!(due(std::slice::from_ref(&r), monday9 - 1).is_empty());
        assert_eq!(due(std::slice::from_ref(&r), monday9), [(1, monday9)]);
        assert_eq!(due(std::slice::from_ref(&r), monday9 + 2), [(1, monday9)], "woken two minutes late");
        assert!(due(std::slice::from_ref(&r), monday9 + 3).is_empty());
        r.last_run = Some(monday9);
        assert!(due(std::slice::from_ref(&r), monday9 + 1).is_empty(), "never twice");
    }

    #[test]
    fn only_level_1_steps_and_a_known_schedule_are_kept() {
        let weekly = || When::Weekly { days: vec![3, 1, 1], time: "09:00".into() };
        let r = validate("  Matin\n", weekly(), vec![step("open_app"), step("web_open")]).unwrap();
        assert_eq!(r.name, "Matin");
        assert_eq!(r.when, When::Weekly { days: vec![1, 3], time: "09:00".into() });
        for tool in ["type_text", "power_action", "create_routine", "system_stats", "clipboard_recent", "prepare_email", "Bash"] {
            assert!(validate("x", weekly(), vec![step(tool)]).is_err(), "{tool}");
        }
        assert!(validate("", weekly(), vec![step("open_app")]).is_err());
        assert!(validate("x", weekly(), vec![]).is_err());
        assert!(validate("x", weekly(), (0..9).map(|_| step("open_app")).collect()).is_err());
        assert!(validate("x", When::Weekly { days: vec![8], time: "09:00".into() }, vec![step("open_app")]).is_err());
        assert!(validate("x", When::Once { date: "2001-01-01".into(), time: "09:00".into() }, vec![step("open_app")]).is_err(), "past");
        let big = Step { tool: "web_open".into(), input: json!({ "query": "x".repeat(2000) }) };
        assert!(validate("x", weekly(), vec![big]).is_err());
        assert!(validate("x", weekly(), vec![Step { tool: "open_app".into(), input: json!("Spotify") }]).is_err());
    }

    #[test]
    fn steps_read_short() {
        let s = Step { tool: "web_open".into(), input: json!({ "site": "amazon", "query": "papier cadeau" }) };
        assert_eq!(step_summary(&s), "web_open · amazon: papier cadeau");
        assert_eq!(step_summary(&Step { tool: "toggle_wifi".into(), input: json!({ "on": false }) }), "toggle_wifi · off");
    }
}
