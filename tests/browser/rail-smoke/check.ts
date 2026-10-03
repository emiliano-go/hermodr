import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { access, mkdtemp } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";

export async function checkRail(endpoint: string, application: string) {
  let session: string | undefined;
  const request = async (method: string, path: string, body?: unknown) => {
    const response = await fetch(`${endpoint}${path}`, { method, signal: AbortSignal.timeout(30_000),
      ...(body ? { headers: { "Content-Type": "application/json" }, body: JSON.stringify(body) } : {}) });
    const result = await response.json();
    if (!response.ok || result.value?.error) throw new Error(JSON.stringify(result.value));
    return result.value;
  };
  const waitFor = async (script: string) => {
    const deadline = Date.now() + 10_000;
    do {
      const value = await request("POST", `/session/${session}/execute/sync`, { script, args: [] });
      if (value) return value;
      await new Promise((resolve) => setTimeout(resolve, 100));
    } while (Date.now() < deadline);
    throw new Error(`Rail fixture timed out: ${script}`);
  };
  const click = async (selector: string) => {
    const element = await request("POST", `/session/${session}/element`, { using: "css selector", value: selector });
    await request("POST", `/session/${session}/element/${element["element-6066-11e4-a52e-4f735466cecf"]}/click`, {});
  };
  try {
    session = (await request("POST", "/session", { capabilities: { alwaysMatch: {
      browserName: "wry", "tauri:options": { application },
    } } })).sessionId;
    assert.ok(session, "WebDriver session missing");
    await waitFor("return !!document.querySelector('#fixture-open')");
    await click("#fixture-open");
    const count = await waitFor("return document.querySelectorAll('.vrow').length");
    assert.ok(count < 30, `Mounted ${count} fixture rows`);
    await click("#fixture-send");
    await waitFor("return !!document.querySelector('[data-id=\"message-400\"]')");
  } finally {
    if (session) await request("DELETE", `/session/${session}`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.platform !== "linux") throw new Error("This manual smoke job requires Linux and WebKitWebDriver");
  const application = resolve("target/rail-smoke/debug/postal");
  await access(application);
  const root = await mkdtemp(join(tmpdir(), "postal-rail-smoke-"));
  const endpoint = "http://127.0.0.1:4444";
  if (await fetch(`${endpoint}/status`, { signal: AbortSignal.timeout(1000) }).then(() => true, () => false)) throw new Error("WebDriver port 4444 already in use");
  const driver = spawn("tauri-driver", [], { stdio: "inherit", env: { ...process.env,
    XDG_DATA_HOME: join(root, "data"), XDG_CONFIG_HOME: join(root, "config"), XDG_CACHE_HOME: join(root, "cache") } });
  let failed: Error | undefined;
  driver.on("error", (error) => { failed = error; });
  driver.on("exit", (code) => { failed ??= new Error(`tauri-driver exited (${code})`); });
  try {
    const deadline = Date.now() + 10_000;
    while (!await fetch(`${endpoint}/status`, { signal: AbortSignal.timeout(1000) }).then((response) => response.ok, () => false)) {
      if (failed) throw failed;
      if (Date.now() >= deadline) throw new Error("tauri-driver failed to start within 10 seconds");
      await new Promise((resolve) => setTimeout(resolve, 100));
    }
    await checkRail(endpoint, application);
    console.log("Tauri rail smoke passed: fixture chat opened, viewport rows mounted, fixture send rendered");
  } finally {
    driver.kill();
    console.log(`Isolated smoke logs retained in ${root}`);
  }
}
