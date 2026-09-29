// One-time migrations from the app's pre-rename identity. Imported first by the
// route so it runs before any state module reads a stored preference.
//
// The bundle-id migration in Rust moves the whole app-data directory, which
// carries the webview's own storage along; these keys are renamed in place.

/** Moves `hermodr.*` localStorage keys to `postal.*`, keeping newer values. */
export function migrateLegacyStorage() {
  try {
    const stale: string[] = [];
    for (let i = 0; i < localStorage.length; i++) {
      const key = localStorage.key(i);
      if (key?.startsWith("hermodr.")) stale.push(key);
    }
    for (const key of stale) {
      const next = `postal.${key.slice("hermodr.".length)}`;
      const value = localStorage.getItem(key);
      if (value !== null && localStorage.getItem(next) === null) localStorage.setItem(next, value);
      localStorage.removeItem(key);
    }
  } catch {
    // Storage may be unavailable; preferences then start fresh.
  }
}

migrateLegacyStorage();
