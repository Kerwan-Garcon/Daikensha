import Foundation
import Combine

// MARK: - Linear models

/// A workflow state of a Linear team ("Todo", "In Progress", "Done"…).
struct LinearState: Identifiable, Equatable, Sendable {
    let id: String
    let name: String
    /// backlog, unstarted, started, completed, canceled or triage.
    let type: String
    let color: String
    let position: Double
}

struct LinearTeam: Identifiable, Equatable, Sendable {
    let id: String
    let key: String
    let name: String
    let states: [LinearState]

    /// The state an issue moves to from `current`: started from backlog/todo/triage,
    /// done from started. Nil once the issue is completed or canceled.
    func nextState(after current: LinearState) -> LinearState? {
        let target: String
        switch current.type {
        case "backlog", "unstarted", "triage": target = "started"
        case "started":                        target = "completed"
        default:                               return nil
        }
        return states.filter { $0.type == target }.min { $0.position < $1.position }
    }
}

struct LinearIssue: Identifiable, Equatable, Sendable {
    let id: String
    let identifier: String      // "ENG-123"
    let title: String
    let url: String
    let priority: Int           // 0 none, 1 urgent … 4 low
    let updatedAt: Date
    let state: LinearState
    let teamId: String
}

// MARK: - Linear settings

enum LinearSettings {
    /// Keychain item holding the personal API key (lin_api_…) or an OAuth token.
    static let keychainKey = "linear-api-key"
    /// Team new issues go to; empty = the user's first team.
    static let teamKey = "linearDefaultTeam"

    static let endpoint = URL(string: "https://api.linear.app/graphql")!
    /// Linear's hosted MCP server, for coding agents (Claude Code, Cursor…).
    static let mcpURL = "https://mcp.linear.app/mcp"
    static let mcpCommand = "claude mcp add --transport http linear \(mcpURL)"

    static var hasKey: Bool { KeychainStore.shared.get(keychainKey) != nil }

    static var defaultTeamId: String {
        UserDefaults.standard.string(forKey: teamKey) ?? ""
    }

    /// Personal API keys go as is; OAuth tokens as a bearer token.
    static func authorization(for key: String) -> String {
        key.hasPrefix("lin_api_") ? key : "Bearer \(key)"
    }
}

// MARK: - Linear Controller

/// The user's open Linear issues, and the two actions the notch offers on them: move an
/// issue to its next state (Todo → In Progress → Done) and create one. Talks to Linear's
/// GraphQL API with the key from Settings. Read every 2 min, only while the Linear pill is
/// on: nothing goes over the network otherwise. Every change to Linear follows a click.
@MainActor
final class LinearController: ObservableObject {
    static let shared = LinearController()
    nonisolated static let pillId = "integration_linear"

    /// Open issues assigned to me, most recently updated first.
    @Published private(set) var issues: [LinearIssue] = []
    @Published private(set) var teams: [LinearTeam] = []
    @Published private(set) var loaded = false
    @Published private(set) var error: String?
    /// Issues being moved, to show a spinner on their row.
    @Published private(set) var updating: Set<String> = []
    @Published private(set) var creating = false
    /// A new issue being typed: holds the island open.
    @Published var composing = false

    private var viewerId: String?
    private var timer: Timer?
    private var fetching = false
    private var lastUpdate: Date = .distantPast
    private var cancellables = Set<AnyCancellable>()

    private var isPillActive: Bool {
        AppState.shared.activeIntegrations.contains(Self.pillId)
    }

    private init() {
        AppState.shared.$activeIntegrations
            .map { $0.contains(Self.pillId) }
            .removeDuplicates()
            .sink { [weak self] _ in
                // The sink runs before the property changes: decide on the next turn.
                DispatchQueue.main.async { self?.updatePolling() }
            }
            .store(in: &cancellables)
    }

    func start() { updatePolling() }

    /// The key changed in Settings: start over with it.
    func keyChanged() {
        issues = []; teams = []; loaded = false; error = nil; viewerId = nil
        lastUpdate = .distantPast
        refresh()
    }

    private func updatePolling() {
        if isPillActive {
            if timer == nil {
                let t = Timer(timeInterval: 120, repeats: true) { _ in
                    MainActor.assumeIsolated { LinearController.shared.refresh() }
                }
                t.tolerance = 20
                RunLoop.main.add(t, forMode: .common)
                timer = t
            }
            refresh()
        } else {
            timer?.invalidate()
            timer = nil
            issues = []; teams = []; loaded = false; error = nil
            lastUpdate = .distantPast
        }
    }

    func refreshIfStale(minAge: TimeInterval = 30) {
        guard Date().timeIntervalSince(lastUpdate) > minAge else { return }
        refresh()
    }

    func refresh() {
        guard isPillActive, !fetching, let key = KeychainStore.shared.get(LinearSettings.keychainKey) else { return }
        fetching = true
        Task {
            defer { fetching = false }
            do {
                let data = try await Self.graphQL(Self.listQuery, variables: [:], key: key)
                let parsed = Self.parse(data)
                viewerId = parsed.viewerId
                if parsed.teams != teams { teams = parsed.teams }
                if parsed.issues != issues { issues = parsed.issues }
                error = nil
                loaded = true
                lastUpdate = Date()
            } catch {
                self.error = error.localizedDescription
                loaded = true
            }
        }
    }

    // MARK: Actions (each one follows a click)

    func team(for issue: LinearIssue) -> LinearTeam? {
        teams.first { $0.id == issue.teamId }
    }

    func nextState(for issue: LinearIssue) -> LinearState? {
        team(for: issue)?.nextState(after: issue.state)
    }

    /// Moves the issue to its next state; a completed one leaves the list.
    func advance(_ issue: LinearIssue) {
        guard let next = nextState(for: issue), !updating.contains(issue.id),
              let key = KeychainStore.shared.get(LinearSettings.keychainKey) else { return }
        updating.insert(issue.id)
        Task {
            defer { updating.remove(issue.id) }
            do {
                let data = try await Self.graphQL(
                    "mutation($id: String!, $stateId: String!) { issueUpdate(id: $id, input: { stateId: $stateId }) { success } }",
                    variables: ["id": issue.id, "stateId": next.id], key: key)
                guard Self.success(data, "issueUpdate") else { throw LinearError.rejected }
                SoundEngine.shared.play(next.type == "completed" ? "approve" : "tick")
                if let i = issues.firstIndex(where: { $0.id == issue.id }) {
                    if next.type == "completed" || next.type == "canceled" {
                        issues.remove(at: i)
                    } else {
                        let old = issues[i]
                        issues[i] = LinearIssue(id: old.id, identifier: old.identifier, title: old.title,
                                                url: old.url, priority: old.priority, updatedAt: Date(),
                                                state: next, teamId: old.teamId)
                    }
                }
                error = nil
            } catch {
                self.error = error.localizedDescription
                SoundEngine.shared.play("error")
            }
        }
    }

    /// Creates an issue assigned to me in the default team. Returns false if it failed.
    func create(title: String) async -> Bool {
        let title = title.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !title.isEmpty, !creating,
              let key = KeychainStore.shared.get(LinearSettings.keychainKey) else { return false }
        let chosen = LinearSettings.defaultTeamId
        guard let team = teams.first(where: { $0.id == chosen }) ?? teams.first else {
            error = String(localized: "linear.no-team")   // key without access to any team
            return false
        }
        creating = true
        defer { creating = false }
        var input: [String: Any] = ["teamId": team.id, "title": String(title.prefix(255))]
        if let viewerId { input["assigneeId"] = viewerId }
        do {
            let data = try await Self.graphQL(
                "mutation($input: IssueCreateInput!) { issueCreate(input: $input) { success } }",
                variables: ["input": input], key: key)
            guard Self.success(data, "issueCreate") else { throw LinearError.rejected }
            SoundEngine.shared.play("approve")
            error = nil
            lastUpdate = .distantPast
            refresh()
            return true
        } catch {
            self.error = error.localizedDescription
            SoundEngine.shared.play("error")
            return false
        }
    }

    // MARK: GraphQL

    enum LinearError: LocalizedError {
        case http(Int), api(String), rejected, offline(String)
        var errorDescription: String? {
            switch self {
            case .http(401), .http(403): return "Invalid API key (401)"
            case .http(let code):        return "API error \(code)"
            case .api(let message):      return message
            case .rejected:              return "Linear refused the change"
            case .offline(let message):  return message
            }
        }
    }

    private static let listQuery = """
    query {
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
    }
    """

    /// Runs a query or mutation; returns the `data` object. Variables carry all user text.
    private static func graphQL(_ query: String, variables: [String: Any],
                                key: String) async throws -> Data {
        var req = URLRequest(url: LinearSettings.endpoint, timeoutInterval: 10)
        req.httpMethod = "POST"
        req.setValue(LinearSettings.authorization(for: key), forHTTPHeaderField: "Authorization")
        req.setValue("application/json", forHTTPHeaderField: "Content-Type")
        req.httpBody = try JSONSerialization.data(withJSONObject: ["query": query, "variables": variables])
        let result: (Data, URLResponse)
        do {
            result = try await URLSession.shared.data(for: req)
        } catch {
            throw LinearError.offline(error.localizedDescription)
        }
        let (data, response) = result
        let code = (response as? HTTPURLResponse)?.statusCode ?? 0
        let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
        if let errors = json?["errors"] as? [[String: Any]], let first = errors.first {
            let ext = first["extensions"] as? [String: Any]
            if (ext?["type"] as? String) == "authentication error" { throw LinearError.http(401) }
            if code == 200 || code == 400 {
                throw LinearError.api((ext?["userPresentableMessage"] as? String)
                                      ?? (first["message"] as? String) ?? "API error")
            }
        }
        guard code == 200 else { throw LinearError.http(code) }
        guard let payload = json?["data"],
              let out = try? JSONSerialization.data(withJSONObject: payload) else { throw LinearError.rejected }
        return out
    }

    private nonisolated static func success(_ data: Data, _ field: String) -> Bool {
        let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any]
        return ((json?[field] as? [String: Any])?["success"] as? Bool) == true
    }

    private nonisolated static func parse(_ data: Data) -> (viewerId: String?, issues: [LinearIssue], teams: [LinearTeam]) {
        guard let json = (try? JSONSerialization.jsonObject(with: data)) as? [String: Any] else {
            return (nil, [], [])
        }
        let viewer = json["viewer"] as? [String: Any]
        let teamNodes = ((json["teams"] as? [String: Any])?["nodes"] as? [[String: Any]]) ?? []
        let teams: [LinearTeam] = teamNodes.compactMap { t in
            guard let id = t["id"] as? String else { return nil }
            let stateNodes = ((t["states"] as? [String: Any])?["nodes"] as? [[String: Any]]) ?? []
            return LinearTeam(id: id, key: t["key"] as? String ?? "", name: t["name"] as? String ?? "",
                              states: stateNodes.compactMap(parseState))
        }
        let issueNodes = ((viewer?["assignedIssues"] as? [String: Any])?["nodes"] as? [[String: Any]]) ?? []
        let issues: [LinearIssue] = issueNodes.compactMap { n in
            guard let id = n["id"] as? String,
                  let state = (n["state"] as? [String: Any]).flatMap(parseState),
                  let teamId = (n["team"] as? [String: Any])?["id"] as? String else { return nil }
            return LinearIssue(id: id,
                               identifier: n["identifier"] as? String ?? "",
                               title: n["title"] as? String ?? "",
                               url: n["url"] as? String ?? "https://linear.app",
                               priority: (n["priority"] as? NSNumber)?.intValue ?? 0,
                               updatedAt: parseDate(n["updatedAt"] as? String) ?? Date(),
                               state: state, teamId: teamId)
        }
        return (viewer?["id"] as? String, issues, teams)
    }

    private nonisolated static func parseState(_ s: [String: Any]) -> LinearState? {
        guard let id = s["id"] as? String else { return nil }
        return LinearState(id: id, name: s["name"] as? String ?? "", type: s["type"] as? String ?? "",
                           color: s["color"] as? String ?? "#8E939C",
                           position: (s["position"] as? NSNumber)?.doubleValue ?? 0)
    }

    private nonisolated static func parseDate(_ s: String?) -> Date? {
        guard let s else { return nil }
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let d = f.date(from: s) { return d }
        f.formatOptions = [.withInternetDateTime]
        return f.date(from: s)
    }

    /// "now", "5m", "3h", "2d".
    static func ago(_ date: Date) -> String {
        let diff = Date().timeIntervalSince(date)
        if diff < 60 { return "now" }
        if diff < 3600 { return "\(Int(diff / 60))m" }
        if diff < 86400 { return "\(Int(diff / 3600))h" }
        return "\(Int(diff / 86400))d"
    }
}
