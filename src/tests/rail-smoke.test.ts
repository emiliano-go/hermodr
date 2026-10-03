import assert from "node:assert/strict";
import test from "node:test";
import { createServer } from "node:http";
import { once } from "node:events";
import { checkRail } from "../../tests/browser/rail-smoke/check.ts";

test("native rail client opens and sends synthetic rows and closes its WebDriver session", async () => {
  const calls: { method: string; url: string; body: any }[] = [];
  const server = createServer(async (request, response) => {
    let text = "";
    for await (const chunk of request) text += chunk;
    const body = text ? JSON.parse(text) : null;
    calls.push({ method: request.method!, url: request.url!, body });
    let value: unknown = null;
    if (request.url === "/session") value = { sessionId: "fixture" };
    else if (request.url?.endsWith("/element")) value = { "element-6066-11e4-a52e-4f735466cecf": body.value };
    else if (request.url?.endsWith("/execute/sync")) value = body.script.includes("querySelectorAll") ? 10 : true;
    response.writeHead(200, { "Content-Type": "application/json" });
    response.end(JSON.stringify({ value }));
  });
  server.listen(0, "127.0.0.1");
  await once(server, "listening");
  try {
    const address = server.address();
    assert.ok(address && typeof address !== "string");
    await checkRail(`http://127.0.0.1:${address.port}`, "/synthetic/postal");
    assert.equal(calls[0].body.capabilities.alwaysMatch["tauri:options"].application, "/synthetic/postal");
    assert.deepEqual(calls.filter((call) => call.url.endsWith("/element")).map((call) => call.body.value), ["#fixture-open", "#fixture-send"]);
    assert.ok(calls.some((call) => call.body?.script?.includes("message-400")));
    assert.equal(calls.at(-1)?.method, "DELETE");
  } finally { await new Promise<void>((resolve) => server.close(() => resolve())); }
});
