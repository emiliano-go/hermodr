import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "vite";
import { fileURLToPath } from "node:url";

test("system themes follow live OS changes while manual and custom themes stay selected", async () => {
  let change: (event: { matches: boolean }) => void = () => {};
  const previous = Object.getOwnPropertyDescriptor(globalThis, "matchMedia");
  Object.defineProperty(globalThis, "matchMedia", { configurable: true, value: (query: string) => ({
    matches: query === "(prefers-color-scheme: dark)",
    addEventListener: (_: string, listener: typeof change) => {
      if (query === "(prefers-color-scheme: dark)") change = listener;
    },
  }) });
  const server = await createServer({
    cacheDir: fileURLToPath(new URL("../../node_modules/.vite-tests/theme", import.meta.url)),
    ssr: { optimizeDeps: { noDiscovery: true, include: [] } },
    server: { middlewareMode: true, ws: false },
  });
  try {
    const theme = await server.ssrLoadModule("/src/lib/utils/theme.svelte.ts");
    assert.equal(theme.customization.theme, "system");
    assert.equal(theme.activeTheme().tokens.scheme, "dark");
    change({ matches: false });
    assert.equal(theme.activeTheme().tokens.scheme, "light");
    assert.ok(theme.isBuiltIn(theme.activeTheme()));
    theme.customization.theme = "dark";
    change({ matches: true });
    change({ matches: false });
    assert.equal(theme.activeTheme().tokens.scheme, "dark");
    const custom = theme.duplicate(theme.activeTheme(), "My theme");
    change({ matches: true });
    assert.equal(theme.activeTheme().id, custom.id);
    assert.ok(["system", "dark", "light"].every((id) => theme.allThemes().some((t: { id: string }) => t.id === id)));
  } finally {
    await server.close();
    if (previous) Object.defineProperty(globalThis, "matchMedia", previous);
    else Reflect.deleteProperty(globalThis, "matchMedia");
  }
});
