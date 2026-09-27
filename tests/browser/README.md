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
