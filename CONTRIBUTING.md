# Contributing to Coucou

Thanks for wanting to help Mochi grow up! 🫶

## Getting started

```bash
brew install xcodegen
cd NotchBuddy && xcodegen && open NotchBuddy.xcodeproj
```

Never edit `NotchBuddy.xcodeproj` by hand: change `project.yml` and run `xcodegen`.

Check resting island dimensions on screens with and without a notch:

```bash
bash scripts/test-screen-geometry.sh
bash scripts/test-display-choice.sh
```

Check auto-close timing and live setting changes:

```bash
bash scripts/test-auto-close.sh
```

## Good first contributions

- A new service integration (a poller + an entry in `PillCatalog.swift` in the `.service` category + a detail card). Look at `StripePoller.swift` for a compact example.
- A new agent: any agent already gets its own automatic pill by sending `coucou_agent` in its hook payload (see `docs/AGENTS.md`). Add an entry in `PillCatalog.swift` in the `.agent` or `.workspace` category only if you want it to be declarable in Settings → Active pills.
- A new emote or sound for Mochi.
- Bug fixes — please describe how to reproduce.

## Rules of the house

- Swift 6, SwiftUI + AppKit, **no third-party dependencies** unless there's really no other way.
- Secrets go in the Keychain, never on disk or in git.
- No telemetry, no network calls except to services the user configured.
- Never block Claude Code: if the app doesn't answer, the hook must exit right away.
- Never write `~/.claude/settings.json` without a backup and the user's confirmation.
- Keep it light: 0 % CPU when the island is hidden.

## Before you push: the checks CI runs

Each stack has one strict configuration, versioned with the code. Run the checks for what you touched; CI fails on any warning.

```bash
# Windows / Linux app (windows/)
npx tsc --noEmit && npm run lint && npm test
cargo build --release -p coucou-hook          # the app bundles it
cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace

# Live Activity relay (relay/)
npm run typecheck && npm test
```

- **TypeScript** compiles with `strict`, `noUncheckedIndexedAccess` and `exactOptionalPropertyTypes`, and lints with typescript-eslint `strictTypeChecked` (`windows/eslint.config.js`). Index with `itemAt` / `cyclicAt` (`core/arrays.ts`) when the bound is known, read untrusted JSON with `text()` (`core/decode.ts`), look DOM elements up with `requireElement` / `requireChild` (`core/elements.ts`) — not with `!`.
- **Rust** follows `[workspace.lints]` in `windows/Cargo.toml`: `clippy::pedantic`, no `unwrap()` outside tests. A new exception goes there with the reason next to it, never as a scattered `#[allow]` without one.
- **IPC**: a new event emitted from Rust gets its payload type in `windows/src/core/events.ts`; a new command gets its entry in `windows/src/core/bridge.ts`.

How the pieces fit: [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md). Code health and what is next: [docs/AUDIT.md](docs/AUDIT.md).

## Pull requests

- One topic per PR, with a short GIF or screenshot for anything visual.
- Build must pass with no new warnings.
