// The Today tab (docs/WINDOWS_SHELL.md): a short to-do list kept on this
// computer, and the weather from Open-Meteo for a city the user typed in
// Settings. No city, no network call; with one, at most a call every thirty
// minutes, and only while the tab is on screen (the tab asks, nothing polls).

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const MAX_TODOS: usize = 50;
const MAX_TODO_CHARS: usize = 200;
const WEATHER_EVERY: Duration = Duration::from_secs(30 * 60);
const MAX_CITY_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Todo {
    pub id: u64,
    pub text: String,
    pub done: bool,
}

/// One line of text, without control characters, cut to `max` characters.
pub fn clean_line(text: &str, max: usize) -> String {
    let flat: String = text.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
    flat.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(max).collect()
}

/// What a file read from disk may hold: bounded, no empty lines, unique ids.
pub fn clean_todos(list: Vec<Todo>) -> Vec<Todo> {
    let mut seen = std::collections::HashSet::new();
    list.into_iter()
        .filter_map(|t| {
            let text = clean_line(&t.text, MAX_TODO_CHARS);
            (!text.is_empty() && seen.insert(t.id)).then_some(Todo { text, ..t })
        })
        .take(MAX_TODOS)
        .collect()
}

fn todos_path() -> std::path::PathBuf {
    crate::settings::config_dir().join("shell-todos.json")
}

pub fn load_todos() -> Vec<Todo> {
    std::fs::read(todos_path())
        .ok()
        .and_then(|b| serde_json::from_slice::<Vec<Todo>>(&b).ok())
        .map(clean_todos)
        .unwrap_or_default()
}

fn save_todos(list: &[Todo]) -> Result<(), String> {
    let path = todos_path();
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let body = serde_json::to_vec_pretty(list).map_err(|e| e.to_string())?;
    std::fs::write(&path, body).map_err(|e| e.to_string())
}

/// What the page may ask of the list.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TodoChange {
    Add { text: String },
    Toggle { id: u64 },
    Remove { id: u64 },
    ClearDone,
}

/// `list` after `change`. Adding to a full list, or an empty line, is refused.
pub fn apply_change(mut list: Vec<Todo>, change: TodoChange) -> Result<Vec<Todo>, String> {
    match change {
        TodoChange::Add { text } => {
            let text = clean_line(&text, MAX_TODO_CHARS);
            if text.is_empty() {
                return Err(crate::i18n::t("Type something first."));
            }
            if list.len() >= MAX_TODOS {
                return Err(crate::i18n::t("The list is full: tick some off first."));
            }
            let id = list.iter().map(|t| t.id).max().map_or(1, |m| m + 1);
            list.push(Todo { id, text, done: false });
        }
        TodoChange::Toggle { id } => {
            if let Some(t) = list.iter_mut().find(|t| t.id == id) {
                t.done = !t.done;
            }
        }
        TodoChange::Remove { id } => list.retain(|t| t.id != id),
        TodoChange::ClearDone => list.retain(|t| !t.done),
    }
    Ok(list)
}

static TODOS_LOCK: Mutex<()> = Mutex::new(());

pub fn change_todos(change: TodoChange) -> Result<Vec<Todo>, String> {
    let _guard = TODOS_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let next = apply_change(load_todos(), change)?;
    save_todos(&next)?;
    Ok(next)
}

// ── Weather ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Weather {
    /// The place Open-Meteo matched ("Bordeaux").
    pub place: String,
    pub temperature: f32,
    pub max: f32,
    pub min: f32,
    /// The WMO weather code; the page names it.
    pub code: u32,
}

struct Cached {
    city: String,
    at: Instant,
    weather: Weather,
}

static CACHE: Mutex<Option<Cached>> = Mutex::new(None);

#[derive(Deserialize)]
struct Geo {
    results: Option<Vec<Place>>,
}

#[derive(Deserialize)]
struct Place {
    name: String,
    latitude: f64,
    longitude: f64,
}

#[derive(Deserialize)]
struct Forecast {
    current: Current,
    daily: Daily,
}

#[derive(Deserialize)]
struct Current {
    temperature_2m: f32,
    weather_code: u32,
}

#[derive(Deserialize)]
struct Daily {
    temperature_2m_max: Vec<f32>,
    temperature_2m_min: Vec<f32>,
}

pub fn geocode_url(city: &str, lang: &str) -> reqwest::Url {
    let mut url = reqwest::Url::parse("https://geocoding-api.open-meteo.com/v1/search").expect("static url");
    url.query_pairs_mut()
        .append_pair("name", city)
        .append_pair("count", "1")
        .append_pair("language", lang)
        .append_pair("format", "json");
    url
}

pub fn forecast_url(lat: f64, lon: f64) -> reqwest::Url {
    let mut url = reqwest::Url::parse("https://api.open-meteo.com/v1/forecast").expect("static url");
    url.query_pairs_mut()
        .append_pair("latitude", &format!("{lat:.3}"))
        .append_pair("longitude", &format!("{lon:.3}"))
        .append_pair("current", "temperature_2m,weather_code")
        .append_pair("daily", "temperature_2m_max,temperature_2m_min")
        .append_pair("timezone", "auto")
        .append_pair("forecast_days", "1");
    url
}

fn parse_place(body: &[u8]) -> Result<Place, String> {
    let geo: Geo = serde_json::from_slice(body).map_err(|e| e.to_string())?;
    geo.results
        .and_then(|r| r.into_iter().next())
        .filter(|p| p.latitude.is_finite() && p.longitude.is_finite())
        .ok_or_else(|| crate::i18n::t("Open-Meteo doesn't know this city."))
}

fn parse_forecast(place: String, body: &[u8]) -> Result<Weather, String> {
    let f: Forecast = serde_json::from_slice(body).map_err(|e| e.to_string())?;
    Ok(Weather {
        place,
        temperature: f.current.temperature_2m,
        max: f.daily.temperature_2m_max.first().copied().unwrap_or(f.current.temperature_2m),
        min: f.daily.temperature_2m_min.first().copied().unwrap_or(f.current.temperature_2m),
        code: f.current.weather_code,
    })
}

async fn get(url: reqwest::Url) -> Result<Vec<u8>, String> {
    let client = crate::net::client(&url, Duration::from_secs(10))?;
    let response = client
        .get(url)
        .send()
        .await
        .map_err(|e| crate::i18n::tf("Network error: {error}", &[("error", &e.to_string())]))?;
    if !response.status().is_success() {
        return Err(crate::i18n::tf("Open-Meteo answered {status}.", &[("status", response.status().as_str())]));
    }
    crate::net::read_capped(response, 64 * 1024).await
}

/// The weather for `city`, from the cache when it is fresh. None when no city is set.
pub async fn weather(city: &str) -> Result<Option<Weather>, String> {
    let city = clean_line(city, MAX_CITY_CHARS);
    if city.is_empty() {
        return Ok(None);
    }
    {
        let cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = cache.as_ref() {
            if c.city == city && c.at.elapsed() < WEATHER_EVERY {
                return Ok(Some(c.weather.clone()));
            }
        }
    }
    let lang = crate::i18n::current().split('-').next().unwrap_or("en").to_string();
    let place = parse_place(&get(geocode_url(&city, &lang)).await?)?;
    let weather = parse_forecast(place.name, &get(forecast_url(place.latitude, place.longitude)).await?)?;
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Cached { city, at: Instant::now(), weather: weather.clone() });
    Ok(Some(weather))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_one_line_and_bounded() {
        assert_eq!(clean_line("  buy\n\tmilk \u{7}", 200), "buy milk");
        assert_eq!(clean_line(&"é".repeat(300), 200).chars().count(), 200);
    }

    #[test]
    fn the_list_adds_toggles_removes_and_refuses_the_rest() {
        let list = apply_change(vec![], TodoChange::Add { text: "a".into() }).unwrap();
        let list = apply_change(list, TodoChange::Add { text: "b".into() }).unwrap();
        assert_eq!(list.iter().map(|t| t.id).collect::<Vec<_>>(), [1, 2]);
        let list = apply_change(list, TodoChange::Toggle { id: 1 }).unwrap();
        assert!(list[0].done);
        let list = apply_change(list, TodoChange::ClearDone).unwrap();
        assert_eq!(list.len(), 1);
        assert!(apply_change(list.clone(), TodoChange::Add { text: " \n ".into() }).is_err());
        let full = (0..MAX_TODOS as u64).map(|id| Todo { id, text: "x".into(), done: false }).collect();
        assert!(apply_change(full, TodoChange::Add { text: "one more".into() }).is_err());
        assert!(apply_change(list, TodoChange::Remove { id: 2 }).unwrap().is_empty());
    }

    #[test]
    fn a_file_on_disk_is_cleaned_before_use() {
        let list = vec![
            Todo { id: 1, text: "ok".into(), done: false },
            Todo { id: 1, text: "same id".into(), done: false },
            Todo { id: 2, text: "   ".into(), done: true },
        ];
        assert_eq!(clean_todos(list).len(), 1);
    }

    #[test]
    fn the_urls_are_open_meteos_and_carry_only_the_city_or_coordinates() {
        let g = geocode_url("Saint-Étienne & co", "fr");
        assert_eq!(g.host_str(), Some("geocoding-api.open-meteo.com"));
        assert!(g.as_str().contains("name=Saint-%C3%89tienne+%26+co"));
        let f = forecast_url(44.837_89, -0.579_18);
        assert_eq!(f.host_str(), Some("api.open-meteo.com"));
        assert!(f.as_str().contains("latitude=44.838&longitude=-0.579"));
    }

    #[test]
    fn open_meteos_answers_are_read() {
        let place = parse_place(br#"{"results":[{"name":"Bordeaux","latitude":44.84,"longitude":-0.58}]}"#).unwrap();
        assert_eq!(place.name, "Bordeaux");
        assert!(parse_place(br#"{"generationtime_ms":0.1}"#).is_err());
        let w = parse_forecast(
            "Bordeaux".into(),
            br#"{"current":{"temperature_2m":17.2,"weather_code":1},"daily":{"temperature_2m_max":[19.0],"temperature_2m_min":[9.1]}}"#,
        )
        .unwrap();
        assert_eq!((w.temperature, w.max, w.min, w.code), (17.2, 19.0, 9.1, 1));
    }
}
