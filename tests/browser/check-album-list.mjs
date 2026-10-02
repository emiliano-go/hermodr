import { spawn } from "node:child_process";
import { randomUUID } from "node:crypto";
import { lstat, mkdir, readFile, realpath, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, isAbsolute, join, resolve } from "node:path";

const tempRoot = await realpath(tmpdir()), profileName = `postal-album-list-${randomUUID()}`, profile = join(tempRoot, profileName);
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
let browser, socket, command, exited, browserExited = false, profileCreated = false;
try {
  await mkdir(profile);
  profileCreated = true;
  browser = spawn("C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe", [
    "--headless=new", "--disable-gpu", "--no-first-run", "--no-default-browser-check", "--remote-debugging-port=0",
    "--disable-background-networking", "--disable-component-update", "--disable-extensions", `--user-data-dir=${profile}`, "about:blank",
  ], { windowsHide: true });
  let spawnError;
  browser.once("error", (error) => { spawnError = error; browserExited = true; });
  exited = new Promise((resolve) => browser.once("exit", () => { browserExited = true; resolve(); }));
  browser.stdout.resume(); browser.stderr.resume();
  const deadline = Date.now() + 45000;
  let endpoint;
  while (!endpoint && Date.now() < deadline) {
    if (spawnError) throw spawnError;
    try {
      const [port, path] = (await readFile(join(profile, "DevToolsActivePort"), "utf8")).trim().split(/\r?\n/);
      if (!/^\d+$/.test(port) || Number(port) < 1 || Number(port) > 65535 || !/^\/devtools\/browser\/[a-zA-Z0-9-]+$/.test(path)) throw new Error("Invalid owned browser endpoint");
      endpoint = `ws://127.0.0.1:${port}${path}`;
    }
    catch { await sleep(100); }
  }
  if (!endpoint) throw new Error("Owned headless Edge endpoint unavailable");
  socket = new WebSocket(endpoint);
  await new Promise((resolve, reject) => {
    socket.addEventListener("open", resolve, { once: true });
    socket.addEventListener("error", reject, { once: true });
  });
  let sequence = 0;
  const pending = new Map(), sessions = new Set(), unexpectedOrigins = new Set();
  function checkRequest(address) {
    try {
      const url = new URL(address);
      if (url.protocol === "data:" || address === "about:blank") return;
      const resource = url.protocol === "blob:" ? new URL(url.pathname) : url;
      if (resource.hostname !== "127.0.0.1" || resource.port !== "1464" || !["http:", "ws:"].includes(resource.protocol)) unexpectedOrigins.add(resource.origin);
    } catch { unexpectedOrigins.add("invalid URL"); }
  }
  socket.addEventListener("message", (event) => {
    const message = JSON.parse(event.data);
    if (sessions.has(message.sessionId)) {
      if (message.method === "Network.requestWillBeSent") checkRequest(message.params.request.url);
      if (message.method === "Network.webSocketCreated") checkRequest(message.params.url);
    }
    const call = pending.get(message.id);
    if (!call) return;
    pending.delete(message.id); clearTimeout(call.timer);
    if (message.error) call.reject(new Error(JSON.stringify(message.error)));
    else call.resolve(message.result);
  });
  const call = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
    const id = ++sequence;
    const timer = setTimeout(() => { pending.delete(id); reject(new Error(`CDP timeout: ${method}`)); }, method === "Runtime.evaluate" ? 15000 : 5000);
    pending.set(id, { resolve, reject, timer });
    socket.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
  });
  command = call;
  for (const width of [400, 320]) {
    const { targetId } = await call("Target.createTarget", { url: "about:blank" });
    const { sessionId } = await call("Target.attachToTarget", { targetId, flatten: true });
    sessions.add(sessionId);
    await call("Network.enable", {}, sessionId);
    await call("Emulation.setDeviceMetricsOverride", { width, height: 900, deviceScaleFactor: 1, mobile: false }, sessionId);
    await call("Page.navigate", { url: "http://127.0.0.1:1464/album-list.html" }, sessionId);
    await call("Page.bringToFront", {}, sessionId);
    let result, pressed = false;
    while (Date.now() < deadline) {
      const read = await call("Runtime.evaluate", { returnByValue: true, expression: `(() => {
        const node = document.querySelector("#album-list-result");
        return node ? JSON.stringify({ viewport: innerWidth, complete: node.dataset.complete, keyboard: node.dataset.keyboard,
          pass: node.dataset.pass, text: node.innerText }) : null;
      })()` }, sessionId);
      if (read.result.value) {
        result = JSON.parse(read.result.value);
        if (result.keyboard === "true" && !pressed) {
          await call("Input.dispatchKeyEvent", { type: "keyDown", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13, nativeVirtualKeyCode: 13, text: "\r" }, sessionId);
          await call("Input.dispatchKeyEvent", { type: "keyUp", key: "Enter", code: "Enter", windowsVirtualKeyCode: 13, nativeVirtualKeyCode: 13 }, sessionId);
          pressed = true;
        }
        if (result.complete === "true") break;
      }
      await sleep(100);
    }
    console.log(JSON.stringify({ requestedWidth: width, pressed, ...result }));
    if (!result || result.complete !== "true" || result.pass !== "true" || result.viewport !== width || !pressed) process.exitCode = 1;
    await call("Target.closeTarget", { targetId });
  }
  if (unexpectedOrigins.size) throw new Error(`Unexpected fixture request origins: ${[...unexpectedOrigins].join(", ")}`);
  console.log(JSON.stringify({ fixtureRequestsOwnedPortOnly: true }));
} catch (error) {
  console.error(String(error)); process.exitCode = 1;
} finally {
  if (command && socket?.readyState === WebSocket.OPEN) await command("Browser.close").catch(() => {});
  socket?.close();
  if (browser && !browserExited) {
    await Promise.race([exited, sleep(5000)]);
    if (!browserExited) { browser.kill(); await Promise.race([exited, sleep(5000)]); }
  }
  if (browser && !browserExited) throw new Error("Owned browser did not exit; profile cleanup skipped");
  browser?.stdout?.destroy(); browser?.stderr?.destroy();
  if (profileCreated) {
    const target = resolve(profile);
    if (!isAbsolute(target) || target !== profile || dirname(target) !== tempRoot || basename(target) !== profileName || !profileName.startsWith("postal-album-list-")) throw new Error("Unsafe owned profile cleanup path");
    const stat = await lstat(target).catch((error) => { if (error.code !== "ENOENT") throw error; });
    if (stat?.isSymbolicLink()) throw new Error("Owned profile cleanup refuses a symlink");
    await rm(target, { recursive: true, force: true, maxRetries: 20, retryDelay: 100 });
    const remains = await lstat(target).then(() => true, (error) => { if (error.code === "ENOENT") return false; throw error; });
    if (remains) throw new Error("Owned profile cleanup failed");
    console.log(JSON.stringify({ profileCleaned: true }));
  }
}
