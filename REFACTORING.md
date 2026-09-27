# Refactoring ledger

Base: `4754994` (`origin/master`, pulled 2026-09-27).
Work happens on `codex/refactor-backend` in an isolated worktree.

## Scope

Move responsibilities out of the route, shell entry point, core message/media
modules, and store initialization. Preserve IPC names, payloads, send order,
history admission, retention timing, saved-name precedence, and quote secrets.
Keep structural changes separate from issue fixes.

| Area | Work | Status |
| --- | --- | --- |
| Frontend | Extract account, finder, attachment and message-action workflows; give theme layers their own component; move shared data types out of views | Implemented; Svelte check passes |
| Shell | Separate account persistence, connection, settings, platform/logging, and command domains | Implemented; 6 tests pass |
| Core | Separate wire-message decoding, media codecs/transfers, and schema setup | Implemented; 78 unit and 10 integration tests pass |
| Preview | Render real app components with inert fixture data; share menu presentation | Implemented; all four scenes checked in browser |
| Backend issues | Recheck #129, #143, #146, #148, #149 against current code and complete acceptance tests | Pending |
| Verification | Core and shell tests, frontend checks/build, IPC registration parity, diff review | In progress |
| Delivery | Signed commits, fast-forward integration with latest master, push, issue comments and closure | Pending |

## Baseline

- `pnpm check`: one pre-existing unused `@ts-expect-error` in `vite.config.js`;
  removed with the frontend cleanup.
- Phone and formatting self-checks pass; production build passes.
- Initial Rust builds exhausted C:; builds rerun with explicit target directories
  on F:. Baseline and extracted code both pass 78 core unit tests,
  10 quote-recovery integration tests and 6 shell tests.
- Refactored frontend: Svelte check 0 errors / 0 warnings; production build,
  phone/format self-checks and base64 binary/chunk-boundary checks pass.
- Browser checks cover preview rendering; live WhatsApp/Tauri IPC was not exercised.

## Follow-up boundaries

Account-generation guards, queued-send lifetime/order, stale chat responses,
frontend resource teardown, history-navigation convergence, atomic account-file
saves and media streaming need behavior-specific regression
proof. Extraction alone does not resolve those audit findings.

Large presentation files are not split merely to reduce line counts. Their
CSS and markup stay with their components unless there is a real shared view.
Changes from other agents must be merged before final validation and push.
