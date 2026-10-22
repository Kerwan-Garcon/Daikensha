import SwiftUI
import AppKit

// MARK: - Linear card

/// My open Linear issues: state, identifier, title. A click on a row opens the issue; the
/// button at its end moves it to its next state (Start, then Done). The field at the bottom
/// creates an issue assigned to me in the team chosen in Settings.
struct LinearCardView: View {
    @ObservedObject private var linear = LinearController.shared
    @State private var draft = ""
    @FocusState private var focused: Bool

    private var accent: Color {
        Color(hex: PillCatalog.definition(for: LinearController.pillId)?.color ?? "#5E6AD2")
    }

    private var teamLabel: String? {
        let chosen = LinearSettings.defaultTeamId
        return (linear.teams.first { $0.id == chosen } ?? linear.teams.first)?.key
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            // Header
            HStack(spacing: 6) {
                Circle()
                    .fill(accent)
                    .frame(width: 7, height: 7)
                Text(verbatim: "Linear")
                    .font(.system(size: 12, weight: .semibold))
                    .foregroundColor(Color(hex: "#F5F6F8"))
                Text("linear.my-issues")
                    .font(.system(size: 11))
                    .foregroundColor(Color(hex: "#8E939C"))
                if !linear.issues.isEmpty {
                    Text(verbatim: "\(linear.issues.count)")
                        .font(.system(size: 10))
                        .foregroundColor(Color(hex: "#6B7079"))
                }
                Spacer(minLength: 2)
            }
            .padding(.top, 6)
            .padding(.leading, 108)
            .padding(.trailing, 36)

            if linear.issues.isEmpty {
                Group {
                    if !linear.loaded {
                        Text("Loading…")
                    } else if let error = linear.error {
                        Text(verbatim: error).foregroundColor(Color(hex: "#F4505E"))
                    } else {
                        Text("linear.no-issues")
                    }
                }
                .font(.system(size: 11))
                .foregroundColor(Color(hex: "#6B7079"))
                .lineLimit(2)
                .padding(.top, 8)
                .padding(.leading, 116)
                .padding(.trailing, 12)
            } else {
                ScrollView(.vertical, showsIndicators: false) {
                    VStack(alignment: .leading, spacing: 2) {
                        ForEach(linear.issues) { issue in
                            LinearIssueRow(issue: issue)
                        }
                    }
                }
                .padding(.top, 5)
                .padding(.leading, 102)
                .padding(.trailing, 12)
            }

            Spacer(minLength: 0)

            // New issue
            if linear.loaded && !linear.teams.isEmpty {
                HStack(spacing: 6) {
                    Image(systemName: "plus")
                        .font(.system(size: 9, weight: .semibold))
                        .foregroundColor(Color(hex: "#6B7079"))
                    TextField(String(format: String(localized: "linear.new-issue %@"), teamLabel ?? ""),
                              text: $draft)
                        .textFieldStyle(.plain)
                        .font(.system(size: 11))
                        .focused($focused)
                        .onSubmit(submit)
                        .onExitCommand { draft = ""; focused = false }
                        .disabled(linear.creating)
                    if linear.creating {
                        ProgressView().controlSize(.mini)
                    } else if !draft.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        Button(action: submit) {
                            Image(systemName: "arrow.up.circle.fill")
                                .font(.system(size: 14))
                                .foregroundColor(accent)
                        }
                        .buttonStyle(.plain)
                    }
                }
                .padding(.horizontal, 8).padding(.vertical, 4)
                .background(RoundedRectangle(cornerRadius: 7).fill(Color.white.opacity(0.06)))
                .padding(.leading, 108)
                .padding(.trailing, 12)
                .padding(.bottom, 4)

                if let error = linear.error, !linear.issues.isEmpty {
                    Text(verbatim: error)
                        .font(.system(size: 9.5))
                        .foregroundColor(Color(hex: "#F4505E"))
                        .lineLimit(1).truncationMode(.tail)
                        .padding(.leading, 116)
                        .padding(.trailing, 12)
                }
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .padding(.top, 4)
        .onAppear { linear.refreshIfStale() }
        .onChange(of: draft) { _, text in
            linear.composing = !text.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        }
        .onDisappear { linear.composing = false }
    }

    private func submit() {
        let title = draft
        guard !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else { return }
        Task {
            if await linear.create(title: title) {
                draft = ""
                focused = false
            }
        }
    }
}

/// One issue: state dot, identifier, title, age, and the button to its next state.
private struct LinearIssueRow: View {
    let issue: LinearIssue
    @ObservedObject private var linear = LinearController.shared
    @State private var hovered = false

    var body: some View {
        HStack(spacing: 6) {
            Button {
                if let url = safeWebURL(issue.url) { NSWorkspace.shared.open(url) }
            } label: {
                HStack(spacing: 6) {
                    Circle()
                        .strokeBorder(Color(hex: issue.state.color), lineWidth: 1.5)
                        .background(Circle().fill(issue.state.type == "started"
                                                  ? Color(hex: issue.state.color).opacity(0.5) : .clear))
                        .frame(width: 9, height: 9)
                        .help(issue.state.name)
                    Text(verbatim: issue.identifier)
                        .font(.system(size: 10, design: .monospaced))
                        .foregroundColor(Color(hex: "#6B7079"))
                        .lineLimit(1)
                        .fixedSize()
                    Text(verbatim: issue.title)
                        .font(.system(size: 11))
                        .foregroundColor(Color(hex: "#C5C8CD"))
                        .lineLimit(1).truncationMode(.tail).layoutPriority(1)
                    Spacer(minLength: 4)
                    Text(verbatim: LinearController.ago(issue.updatedAt))
                        .font(.system(size: 9))
                        .foregroundColor(Color(hex: "#4B5563"))
                }
                .contentShape(Rectangle())
            }
            .buttonStyle(.plain)

            if linear.updating.contains(issue.id) {
                ProgressView().controlSize(.mini).frame(width: 44)
            } else if let next = linear.nextState(for: issue) {
                Button {
                    linear.advance(issue)
                } label: {
                    Text(next.type == "completed" ? LocalizedStringKey("linear.done") : LocalizedStringKey("linear.start"))
                        .font(.system(size: 9.5, weight: .medium))
                        .foregroundColor(Color(hex: next.color))
                        .padding(.horizontal, 6).padding(.vertical, 2)
                        .background(Capsule().fill(Color(hex: next.color).opacity(0.14)))
                        .frame(width: 44)
                }
                .buttonStyle(.plain)
                .help(next.name)
            }
        }
        .padding(.horizontal, 6).padding(.vertical, 3)
        .background(RoundedRectangle(cornerRadius: 6).fill(Color.white.opacity(hovered ? 0.05 : 0)))
        .onHover { hovered = $0 }
    }
}
