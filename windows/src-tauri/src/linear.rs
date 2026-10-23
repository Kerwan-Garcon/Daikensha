// Linear — the Rust side of LinearController.swift.
//
// The user's open issues, read every 2 min while the pill is on (integrations.rs
// spawns the poller), and the two actions the island offers on them: move an
// issue to its next state (Todo → In Progress → Done) and create one. Same
// GraphQL API and the same key as the Mac. Every change to Linear follows a
// click; the text the user typed only ever travels in GraphQL variables.

use parking_lot::Mutex;
use serde::Serialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Manager};

use crate::integrations::{self, IntegrationUpdate};
use crate::secrets;

pub const ID: &str = "integration_linear";
pub const KEY: &str = "linear-api-key";
const ENDPOINT: &str = "https://api.linear.app/graphql";

const LIST_QUERY: &str = r#"query {
  viewer {
    id
    assignedIssues(first: 20, orderBy: updatedAt,
                   filter: { state: { type: { nin: ["completed", "canceled"] } } }) {
      nodes { id identifier title url priority updatedAt team { id }
              state { id name type color position } }
    }
  }
  teams(first: 50) {
    nodes { id key name states { nodes { id name type color position } } }
  }
}"#;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct State {
    pub id: String,
    pub name: String,
    /// backlog, unstarted, started, completed, canceled or triage.
    #[serde(rename = "type")]
    pub kind: String,
    pub color: String,
    #[serde(skip)]
    pub position: f64,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Team {
    pub id: String,
    pub key: String,
    pub name: String,
    #[serde(skip)]
    pub states: Vec<State>,
}

impl Team {
    /// The state an issue moves to from `current`: started from backlog/todo/
    /// triage, done from started. None once it is completed or canceled.
    pub fn next_state(&self, current: &State) -> Option<&State> {
        let target = match current.kind.as_str() {
            "backlog" | "unstarted" | "triage" => "started",
            "started" => "completed",
            _ => return None,
        };
        self.states
            .iter()
            .filter(|s| s.kind == target)
            .min_by(|a, b| a.position.total_cmp(&b.position))
    }
}

/// What the last read learnt, for the actions: who I am and the teams.
#[derive(Default)]
struct Cache {
    viewer: Option<String>,
    teams: Vec<Team>,
}

static CACHE: Mutex<Cache> = Mutex::new(Cache { viewer: None, teams: Vec::new() });

/// Personal API keys go as they are; OAuth tokens as a bearer token.
fn authorization(key: &str) -> String {
    if key.starts_with("lin_api_") { key.to_string() } else { format!("Bearer {key}") }
}

/// Runs a query or mutation and returns its `data`, or the line the card shows.
async fn graphql(key: &str, query: &str, variables: Value) -> Result<Value, String> {
    let response = integrations::client()
        .post(ENDPOINT)
        .header("Authorization", authorization(key))
        .header("Content-Type", "application/json")
        .json(&json!({ "query": query, "variables": variables }))
        .send()
        .await
        .map_err(|e| crate::i18n::tf("No connection: {error}", &[("error", &e.to_string())]))?;
    let code = response.status().as_u16();
    let body: Value = response.json().await.unwrap_or(json!({}));
    if let Some(error) = body.get("errors").and_then(Value::as_array).and_then(|a| a.first()) {
        let ext = error.get("extensions");
        if ext.and_then(|e| e.get("type")).and_then(Value::as_str) == Some("authentication error") {
            return Err(crate::i18n::t("Invalid API key (401)"));
        }
        if code == 200 || code == 400 {
            let message = ext
                .and_then(|e| e.get("userPresentableMessage"))
                .or_else(|| error.get("message"))
                .and_then(Value::as_str)
                .unwrap_or("API error");
            return Err(message.to_string());
        }
    }
    if code != 200 {
        return Err(integrations::status_error(code, &crate::i18n::t("Invalid API key (401)")));
    }
    body.get("data").cloned().ok_or_else(|| crate::i18n::t("Linear refused the change"))
}

fn parse_state(v: &Value) -> Option<State> {
    Some(State {
        id: v.get("id")?.as_str()?.to_string(),
        name: v.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
        kind: v.get("type").and_then(Value::as_str).unwrap_or("").to_string(),
        color: v.get("color").and_then(Value::as_str).unwrap_or("#8E939C").to_string(),
        position: v.get("position").and_then(Value::as_f64).unwrap_or(0.0),
    })
}

fn nodes(v: Option<&Value>) -> &[Value] {
    v.and_then(|v| v.get("nodes")).and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[])
}

fn parse_teams(data: &Value) -> Vec<Team> {
    nodes(data.get("teams"))
        .iter()
        .filter_map(|t| {
            Some(Team {
                id: t.get("id")?.as_str()?.to_string(),
                key: t.get("key").and_then(Value::as_str).unwrap_or("").to_string(),
                name: t.get("name").and_then(Value::as_str).unwrap_or("").to_string(),
                states: nodes(t.get("states")).iter().filter_map(parse_state).collect(),
            })
        })
        .collect()
}

/// The card's data: each issue with the state its button moves it to.
fn card_data(data: &Value, teams: &[Team]) -> Value {
    let issues: Vec<Value> = nodes(data.get("viewer").and_then(|v| v.get("assignedIssues")))
        .iter()
        .filter_map(|n| {
            let state = parse_state(n.get("state")?)?;
            let team_id = n.get("team")?.get("id")?.as_str()?;
            let next = teams.iter().find(|t| t.id == team_id).and_then(|t| t.next_state(&state)).cloned();
            Some(json!({
                "id": n.get("id")?.as_str()?,
                "identifier": n.get("identifier").and_then(Value::as_str).unwrap_or(""),
                "title": n.get("title").and_then(Value::as_str).unwrap_or(""),
                "url": n.get("url").and_then(Value::as_str).unwrap_or("https://linear.app"),
                "updatedAt": n.get("updatedAt").and_then(Value::as_str).unwrap_or(""),
                "state": state,
                "next": next,
            }))
        })
        .collect();
    json!({ "issues": issues, "teams": teams })
}

pub async fn poll(app: AppHandle) {
    let Some(key) = secrets::get(KEY) else { return };
    match graphql(&key, LIST_QUERY, json!({})).await {
        Ok(data) => {
            let teams = parse_teams(&data);
            let card = card_data(&data, &teams);
            {
                let mut cache = CACHE.lock();
                cache.viewer = data
                    .get("viewer")
                    .and_then(|v| v.get("id"))
                    .and_then(Value::as_str)
                    .map(str::to_string);
                cache.teams = teams;
            }
            integrations::emit(&app, IntegrationUpdate { id: ID, data: card, error: None, event: None });
        }
        Err(error) => {
            integrations::emit(&app, IntegrationUpdate { id: ID, data: json!({}), error: Some(error), event: None });
        }
    }
}

/// Teams the key can see, for Settings → Integrations → Linear.
pub async fn teams() -> Vec<Team> {
    let cached = CACHE.lock().teams.clone();
    if !cached.is_empty() {
        return cached;
    }
    let Some(key) = secrets::get(KEY) else { return Vec::new() };
    match graphql(&key, "query { teams(first: 50) { nodes { id key name } } }", json!({})).await {
        Ok(data) => parse_teams(&data),
        Err(_) => Vec::new(),
    }
}

/// The team picked in Settings, else the first one.
fn chosen_team(teams: &[Team], picked: &str) -> Option<String> {
    teams.iter().find(|t| t.id == picked).or_else(|| teams.first()).map(|t| t.id.clone())
}

fn succeeded(data: &Value, field: &str) -> bool {
    data.get(field).and_then(|f| f.get("success")).and_then(Value::as_bool) == Some(true)
}

/// Moves an issue to the state its button showed, then reads the list again.
pub async fn advance(app: AppHandle, issue_id: String, state_id: String) -> Result<(), String> {
    let key = secrets::get(KEY).ok_or_else(|| crate::i18n::t("Key not configured"))?;
    let data = graphql(
        &key,
        "mutation($id: String!, $stateId: String!) { issueUpdate(id: $id, input: { stateId: $stateId }) { success } }",
        json!({ "id": issue_id, "stateId": state_id }),
    )
    .await?;
    if !succeeded(&data, "issueUpdate") {
        return Err(crate::i18n::t("Linear refused the change"));
    }
    poll(app).await;
    Ok(())
}

/// Creates an issue assigned to me in the chosen team, then reads the list again.
pub async fn create(app: AppHandle, title: String) -> Result<(), String> {
    let title: String = title.trim().chars().take(255).collect();
    if title.is_empty() {
        return Err(crate::i18n::t("Linear refused the change"));
    }
    let key = secrets::get(KEY).ok_or_else(|| crate::i18n::t("Key not configured"))?;
    let picked = app
        .try_state::<crate::Shared>()
        .map(|shared| shared.settings.lock().linear_team.clone())
        .unwrap_or_default();
    let (team, viewer) = {
        let cache = CACHE.lock();
        (chosen_team(&cache.teams, &picked), cache.viewer.clone())
    };
    let team = team.ok_or_else(|| crate::i18n::t("This key can't see any Linear team"))?;
    let mut input = json!({ "teamId": team, "title": title });
    if let Some(viewer) = viewer {
        input["assigneeId"] = json!(viewer);
    }
    let data = graphql(
        &key,
        "mutation($input: IssueCreateInput!) { issueCreate(input: $input) { success } }",
        json!({ "input": input }),
    )
    .await?;
    if !succeeded(&data, "issueCreate") {
        return Err(crate::i18n::t("Linear refused the change"));
    }
    poll(app).await;
    Ok(())
}

/// A new key: forget what the old one saw.
/// The key was saved or removed (call it after the store is written): forget
/// what the old key saw and show the new state at once, as the Mac's
/// keyChanged() does, instead of waiting for the next poll — which never comes
/// once the key is gone.
pub fn key_changed(app: &AppHandle) {
    {
        let mut cache = CACHE.lock();
        cache.viewer = None;
        cache.teams.clear();
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if secrets::present(KEY) {
            poll(app).await;
        } else {
            integrations::emit(&app, IntegrationUpdate { id: ID, data: json!({}), error: None, event: None });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(id: &str, kind: &str, position: f64) -> State {
        State { id: id.into(), name: id.into(), kind: kind.into(), color: "#000000".into(), position }
    }

    fn team() -> Team {
        Team {
            id: "t1".into(),
            key: "ENG".into(),
            name: "Engineering".into(),
            states: vec![
                state("backlog", "backlog", 0.0),
                state("todo", "unstarted", 1.0),
                state("review", "started", 3.0),
                state("doing", "started", 2.0),
                state("done", "completed", 4.0),
                state("shipped", "completed", 5.0),
                state("nope", "canceled", 6.0),
            ],
        }
    }

    #[test]
    fn the_next_state_is_the_first_started_then_the_first_completed() {
        let team = team();
        assert_eq!(team.next_state(&state("todo", "unstarted", 1.0)).unwrap().id, "doing");
        assert_eq!(team.next_state(&state("backlog", "backlog", 0.0)).unwrap().id, "doing");
        assert_eq!(team.next_state(&state("x", "triage", 0.0)).unwrap().id, "doing");
        assert_eq!(team.next_state(&state("doing", "started", 2.0)).unwrap().id, "done");
        assert!(team.next_state(&state("done", "completed", 4.0)).is_none());
        assert!(team.next_state(&state("nope", "canceled", 6.0)).is_none());
    }

    #[test]
    fn personal_keys_go_as_they_are_and_tokens_as_bearer() {
        assert_eq!(authorization("lin_api_abc"), "lin_api_abc");
        assert_eq!(authorization("oauth-token"), "Bearer oauth-token");
    }

    #[test]
    fn the_picked_team_else_the_first() {
        let teams = vec![team(), Team { id: "t2".into(), ..team() }];
        assert_eq!(chosen_team(&teams, "t2").as_deref(), Some("t2"));
        assert_eq!(chosen_team(&teams, "").as_deref(), Some("t1"));
        assert_eq!(chosen_team(&teams, "gone").as_deref(), Some("t1"));
        assert_eq!(chosen_team(&[], ""), None);
    }

    #[test]
    fn the_card_gets_each_issue_with_its_next_state() {
        let data = json!({
            "viewer": { "id": "me", "assignedIssues": { "nodes": [
                { "id": "i1", "identifier": "ENG-1", "title": "Fix it", "url": "https://linear.app/x/ENG-1",
                  "updatedAt": "2026-10-20T10:00:00.000Z", "team": { "id": "t1" },
                  "state": { "id": "todo", "name": "Todo", "type": "unstarted", "color": "#e2e2e2", "position": 1 } },
                { "id": "broken" }
            ] } },
            "teams": { "nodes": [ { "id": "t1", "key": "ENG", "name": "Engineering", "states": { "nodes": [
                { "id": "doing", "name": "In Progress", "type": "started", "color": "#f2c94c", "position": 2 }
            ] } } ] }
        });
        let teams = parse_teams(&data);
        let card = card_data(&data, &teams);
        let issues = card["issues"].as_array().unwrap();
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0]["identifier"], "ENG-1");
        assert_eq!(issues[0]["state"]["type"], "unstarted");
        assert_eq!(issues[0]["next"]["id"], "doing");
        assert_eq!(issues[0]["next"]["name"], "In Progress");
        assert_eq!(card["teams"], json!([{ "id": "t1", "key": "ENG", "name": "Engineering" }]));
    }
}
