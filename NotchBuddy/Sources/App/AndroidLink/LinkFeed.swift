#if ANDROID_LINK
import AppKit
import Combine

// MARK: - Android link: what the island knows, published to the hub; a phone's decision, applied as a click
//
// The same data the iPhone's SessionPublisher reads (AppState), with the same guards (no demo cards,
// no services as sessions), published to the hub (Core/LinkHub.swift) instead of iCloud. Driven by
// AppState changes only, never by a timer. The hub works out what changed and tells the phones.

@MainActor
final class LinkFeed {
    private let hub: LinkHub
    private var cancellable: AnyCancellable?

    init(hub: LinkHub) {
        self.hub = hub
    }

    func start() {
        guard cancellable == nil else { return }
        let state = AppState.shared
        // @Published sends the new value before the property changes, so the values are passed along.
        cancellable = state.$tasks
            .combineLatest(state.$pendingApproval.removeDuplicates { $0?.id == $1?.id })
            .debounce(for: .milliseconds(150), scheduler: RunLoop.main)
            .sink { [weak self] tasks, approval in
                MainActor.assumeIsolated { self?.publish(tasks: tasks, approval: approval) }
            }
        publish(tasks: state.tasks, approval: state.pendingApproval)
    }

    func stop() {
        cancellable = nil
    }

    private func publish(tasks: [AgentTask], approval: ApprovalInfo?) {
        // The demo's sessions and cards are for the island only, as for the iPhone.
        guard !DemoEngine.shared.isActive else { return }
        hub.publish(sessions: Self.sessions(tasks), approval: Self.approval(approval), now: LinkProtocol.nowMs())
    }

    /// The agents' sessions, not the services (they have their own capability).
    static func sessions(_ tasks: [AgentTask]) -> [SessionIn] {
        tasks.filter { $0.source != .n8n && PillCatalog.isSession($0.id) }.map { task in
            let last = task.steps.last ?? ""
            // A step that stands for a file change is shown by the file's name, as on the island.
            let status = last.parseDiffStep()?.filename ?? last
            return SessionIn(pillId: task.id, agent: task.name, state: task.state.rawValue, statusText: status,
                             stepIndex: task.stepIndex, stepCount: task.steps.count)
        }
    }

    /// The request on the approval card. Its id is the fingerprint's input key: two requests for the same
    /// command are two fingerprints, and a late decision can never answer the next one.
    static func approval(_ a: ApprovalInfo?) -> ApprovalIn? {
        guard let a, a.sessionId != "demo_session" else { return nil }
        return ApprovalIn(requestId: a.id.uuidString, sessionId: a.sessionId, pillId: a.pillId, tool: a.tool, command: a.command)
    }
}

/// A phone's decision, applied exactly as a click on the island's card: only for the request still on it.
struct MacLinkHost: LinkHost {
    func decide(requestId: String, allow: Bool) {
        Task { @MainActor in
            // Checked again on the main actor, right before answering the hook.
            guard let pending = AppState.shared.pendingApproval, pending.id.uuidString == requestId else {
                linkLog("decision arrived after the request was resolved, ignored")
                return
            }
            linkLog("\(allow ? "allowed" : "denied") from the Android phone: \(pending.tool)")
            HookServer.shared.sendApprovalDecision(allow ? "allow" : "deny", for: pending.id)
        }
    }
}
#endif
