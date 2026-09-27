# Synthetic browser checks

Run `pnpm exec vite --config tests/browser/vite.config.ts` and open
`http://127.0.0.1:1432` in an isolated browser tab.
This harness imports production components but replaces IPC with synthetic
responses. Unknown commands fail; no account, database or native API is opened.
The harness is separate from the production SvelteKit routes.

Flag diagnostics checks:

1. Expand Server feature flags: true, false and Not received have separate values.
2. Filter by name or code; clear the filter to see all rows.
3. Enable Watch, apply the synthetic delta, then wait five seconds: enabled turns false.
4. Simulate failure and refresh: error is visible and previous values remain.
5. Toggle diagnostics off and Count reads. Wait over five seconds and count again:
   the count must stay unchanged.

Chat metadata check: expand Retained chat metadata. Quiet contact must remain
pinned with its last activity and "No stored messages"; selecting it must set
the displayed selection. No expired message preview is supplied to the component.

Retention checks: open per-chat retention. Default means inherit; Forever and
No limit save unlimited tags. A one-week age with Default count saves
limited(168) and inherit. Open global retention, clear both numeric fields and
save: both limits must be unlimited. Saved payloads appear below the launch buttons.

Storage Manager checks: expand the manager, filter Synthetic A and switch between
Largest first and Oldest first. Cancel a deletion and confirm cleanup-call count
stays zero; confirm a deletion and verify updated totals/list. Clean chat media
and clear the picture cache independently. Toggle cleanup failure and attempt
another deletion: error must stay visible and file/totals remain unchanged.
