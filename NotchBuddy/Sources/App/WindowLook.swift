#if !APPSTORE
import AppKit
import SwiftUI

/// Drop Mochi from the island onto a window: he looks at it and says what he sees, a summary
/// and a short analysis, in the island's chat. The chips under the chat ask for more (a summary,
/// an analysis, the key points, a translation), and the chat goes on as usual.
///
/// Only the window Mochi lands on is captured (screencapture -l), shrunk, and sent to the chat
/// AI the user configured, nowhere else. Holding ⌥ while dropping only attaches the window to
/// the chat, as before.
@MainActor
final class WindowLook {
    static let shared = WindowLook()
    private init() {}

    private var running = false

    private static var folder: URL {
        FileManager.default.temporaryDirectory.appendingPathComponent("CoucouLooks", isDirectory: true)
    }

    /// The longest side sent to the AI: Claude scales bigger images down to this anyway.
    private static let maxSide: CGFloat = 1568

    /// The follow-ups offered under the chat once Mochi has looked at a window.
    enum Ask: CaseIterable {
        case summary, analysis, keyPoints, translate

        var label: String {
            switch self {
            case .summary:   String(localized: "look.chip.summary")
            case .analysis:  String(localized: "look.chip.analysis")
            case .keyPoints: String(localized: "look.chip.key-points")
            case .translate: String(localized: "look.chip.translate")
            }
        }

        /// What is sent, as if the user had typed it.
        var query: String {
            switch self {
            case .summary:   String(localized: "look.ask.summary")
            case .analysis:  String(localized: "look.ask.analysis")
            case .keyPoints: String(localized: "look.ask.key-points")
            case .translate: String(localized: "look.ask.translate")
            }
        }
    }

    // MARK: - Looking

    /// Mochi was dropped at `screenPoint` (AppKit coordinates) on the window `context` describes.
    /// True: the island is to open on the chat, where the answer comes. False: it shows a note
    /// (Screen Recording permission missing).
    @discardableResult
    func look(at screenPoint: NSPoint, context: PromptContext) -> Bool {
        guard !running else { return true }
        let state = AppState.shared
        state.promptContext = context
        guard CGPreflightScreenCaptureAccess() else {
            appendAppLog("nb.log", "[Look] no Screen Recording permission: asking")
            // macOS asks once; then Coucou is to be allowed in Privacy → Screen Recording.
            _ = CGRequestScreenCaptureAccess()
            state.noteMessage = String(localized: "look.permission")
            return false
        }
        // No window found again: the window is only attached, as with ⌥.
        guard let target = Self.window(at: screenPoint) else { return true }
        running = true

        // A new conversation about this window.
        state.chatHistory = [ChatMessage(role: .user, content: String(localized: "look.bubble"))]
        ClaudeService.shared.clearConversation()
        state.stateOverride = .thinking
        NotificationCenter.default.post(name: .triggerEmote, object: BotEmote.listening)

        Task { @MainActor in
            defer { self.running = false }
            let shot = await Self.capture(windowID: target.id)
            guard let shot else {
                state.stateOverride = nil
                state.chatHistory = []
                state.noteMessage = String(localized: "look.failed")
                NotificationCenter.default.post(name: .hookExpand, object: IslandView.note)
                return
            }
            var ctx = context
            if case .window(let app, let title, let url, _) = context {
                // The window under Mochi, which is not always the app's focused one.
                ctx = .window(appName: app, title: target.title ?? title, url: url, shot: shot)
            }
            state.promptContext = ctx
            SoundEngine.shared.play("gulp")
            await ClaudeService.shared.chat(query: Self.lookPrompt, context: ctx, state: state)
        }
        return true
    }

    /// One of the chips: asked in the same conversation, with the window still attached.
    func ask(_ ask: Ask) {
        let state = AppState.shared
        guard state.stateOverride == nil else { return }
        state.chatHistory.append(ChatMessage(role: .user, content: ask.query))
        state.stateOverride = .thinking
        Task { @MainActor in
            await ClaudeService.shared.chat(query: ask.query, context: state.promptContext, state: state)
        }
    }

    /// What Mochi is asked to do with the window. In English (like the other prompts), with the
    /// reply in the app's language since the user has not typed anything yet.
    private static var lookPrompt: String {
        let code = Bundle.main.preferredLocalizations.first ?? "en"
        let language = Locale(identifier: "en").localizedString(forIdentifier: code) ?? "English"
        return """
        The user just dropped you onto this window of their Mac: the screenshot attached is what \
        you are looking at. Tell them what you see. Start with one sentence saying what it is, \
        then a short summary of its content, then your analysis: what stands out, anything wrong \
        or worth checking, and what they could do next. Stay short and concrete, quote what you \
        read when it helps. Do not describe the window's chrome (toolbars, tabs) unless it matters. \
        Reply in \(language).
        """
    }

    // MARK: - The window under Mochi

    private struct Target {
        let id: CGWindowID
        let title: String?
    }

    /// The topmost window of another app under `screenPoint` (AppKit coordinates).
    private static func window(at screenPoint: NSPoint) -> Target? {
        // CGWindowList is y-down from the top of the menu-bar screen.
        let cgPoint = DesktopSpace.topDown(screenPoint, desktopTop: IslandWindowController.desktopTop)
        guard let list = CGWindowListCopyWindowInfo(
            [.optionOnScreenOnly, .excludeDesktopElements], kCGNullWindowID
        ) as? [[String: Any]] else { return nil }
        let ours = Bundle.main.bundleIdentifier ?? ""
        for info in list {
            guard let b = info[kCGWindowBounds as String] as? [String: Any],
                  let x = b["X"] as? CGFloat, let y = b["Y"] as? CGFloat,
                  let w = b["Width"] as? CGFloat, let h = b["Height"] as? CGFloat,
                  CGRect(x: x, y: y, width: w, height: h).contains(cgPoint) else { continue }
            let pid = info[kCGWindowOwnerPID as String] as? pid_t ?? 0
            guard let app = NSRunningApplication(processIdentifier: pid),
                  app.bundleIdentifier != ours,
                  app.activationPolicy == .regular,
                  let number = info[kCGWindowNumber as String] as? Int else { continue }
            let name = (info[kCGWindowName as String] as? String).flatMap { $0.isEmpty ? nil : $0 }
            return Target(id: CGWindowID(number), title: name)
        }
        return nil
    }

    // MARK: - Capturing the window

    /// The window alone (even partly covered), as a JPEG no bigger than `maxSide`. Nil if it failed.
    private static func capture(windowID: CGWindowID) async -> URL? {
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        prune()
        let stamp = Int(Date().timeIntervalSince1970 * 1000)
        let raw = folder.appendingPathComponent("look-\(stamp).png")
        let ok: Bool = await withCheckedContinuation { done in
            let task = Process()
            task.executableURL = URL(fileURLWithPath: "/usr/sbin/screencapture")
            // -l: that window only; -x: no shutter sound; -o: no window shadow.
            task.arguments = ["-l\(windowID)", "-x", "-o", raw.path]
            task.terminationHandler = { p in done.resume(returning: p.terminationStatus == 0) }
            do { try task.run() } catch {
                appendAppLog("nb.log", "[Look] screencapture failed: \(error)")
                done.resume(returning: false)
            }
        }
        defer { try? FileManager.default.removeItem(at: raw) }
        guard ok, let image = NSImage(contentsOf: raw),
              let cg = image.cgImage(forProposedRect: nil, context: nil, hints: nil) else {
            appendAppLog("nb.log", "[Look] no image for window \(windowID)")
            return nil
        }
        let file = folder.appendingPathComponent("look-\(stamp).jpg")
        guard let jpeg = jpegData(cg), (try? jpeg.write(to: file, options: .atomic)) != nil else { return nil }
        appendAppLog("nb.log", "[Look] window \(windowID) captured (\(cg.width)×\(cg.height))")
        return file
    }

    private static func jpegData(_ cg: CGImage) -> Data? {
        let w = CGFloat(cg.width), h = CGFloat(cg.height)
        let scale = min(1, maxSide / max(w, h))
        let size = CGSize(width: max(1, (w * scale).rounded()), height: max(1, (h * scale).rounded()))
        guard let ctx = CGContext(data: nil, width: Int(size.width), height: Int(size.height),
                                  bitsPerComponent: 8, bytesPerRow: 0,
                                  space: CGColorSpaceCreateDeviceRGB(),
                                  bitmapInfo: CGImageAlphaInfo.noneSkipLast.rawValue) else { return nil }
        ctx.interpolationQuality = .high
        ctx.setFillColor(CGColor(gray: 1, alpha: 1))
        ctx.fill(CGRect(origin: .zero, size: size))
        ctx.draw(cg, in: CGRect(origin: .zero, size: size))
        guard let scaled = ctx.makeImage() else { return nil }
        return NSBitmapImageRep(cgImage: scaled).representation(using: .jpeg, properties: [.compressionFactor: 0.82])
    }

    /// Keeps the last 10 looks.
    private static func prune() {
        let fm = FileManager.default
        guard let files = try? fm.contentsOfDirectory(at: folder, includingPropertiesForKeys: [.creationDateKey]) else { return }
        let sorted = files.sorted {
            let a = (try? $0.resourceValues(forKeys: [.creationDateKey]).creationDate) ?? .distantPast
            let b = (try? $1.resourceValues(forKeys: [.creationDateKey]).creationDate) ?? .distantPast
            return a > b
        }
        for old in sorted.dropFirst(10) { try? fm.removeItem(at: old) }
    }
}

// MARK: - Chips under the chat

/// Summary, analysis, key points, translation: shown under the chat once Mochi has looked at a window.
struct LookChips: View {
    @ObservedObject var state: AppState

    var body: some View {
        HStack(spacing: 5) {
            ForEach(WindowLook.Ask.allCases, id: \.self) { ask in
                Button { WindowLook.shared.ask(ask) } label: {
                    Text(verbatim: ask.label)
                        .font(.system(size: 10.5, weight: .medium))
                        .foregroundColor(Color(hex: "#C9CCD2"))
                        .lineLimit(1)
                        .fixedSize()
                        .padding(.horizontal, 8)
                        .padding(.vertical, 4)
                        .background(Color.white.opacity(0.06))
                        .clipShape(Capsule())
                }
                .buttonStyle(.plain)
                .disabled(state.stateOverride != nil)
                .opacity(state.stateOverride != nil ? 0.5 : 1)
            }
        }
    }
}
#endif
