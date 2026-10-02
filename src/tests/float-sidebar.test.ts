import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import ts from "typescript";

const source = readFileSync(new URL("../lib/chat/ChatSidebar.svelte", import.meta.url), "utf8");
const script = source.match(/<script[^>]*>([\s\S]*?)<\/script>/)![1];
const tree = ts.createSourceFile("sidebar.ts", script, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
const declaration = tree.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === "floatChat")!;
const code = ts.transpileModule(declaration.getText(tree), {
  compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext },
}).outputText;

function fixture() {
  let finish!: () => void, fail!: (error: unknown) => void;
  const response = new Promise<void>((resolve, reject) => { finish = resolve; fail = reject; });
  const calls: unknown[] = [];
  let closed = 0;
  const controller = new Function("invoke", "closeChatMenu", `
    let activeAccount = "a", menuRequest = 1, floatBusy = false, floatError = null;
    ${code}
    return { floatChat, switchAccount: () => { activeAccount = "b"; },
      reopen: () => { menuRequest++; floatBusy = false; floatError = null; },
      state: () => ({ floatBusy, floatError }) };
  `)((command: string, args: unknown) => { calls.push({ command, args }); return response; },
    () => { closed++; }) as {
      floatChat(chat: string): Promise<void>; switchAccount(): void; reopen(): void;
      state(): { floatBusy: boolean; floatError: string | null };
    };
  return { controller, calls, finish, fail, closed: () => closed };
}

test("Float chat captures account and target and ignores duplicate clicks", async () => {
  const f = fixture();
  const opening = f.controller.floatChat("room@g.us");
  await f.controller.floatChat("different@g.us");
  assert.deepEqual(f.calls, [{ command: "open_float_chat", args: { accountId: "a", chat: "room@g.us" } }]);
  f.finish(); await opening;
  assert.equal(f.closed(), 1);
  assert.equal(f.controller.state().floatBusy, false);
});

test("late opener success cannot close another account menu", async () => {
  const f = fixture(), opening = f.controller.floatChat("room@g.us");
  f.controller.switchAccount(); f.controller.reopen();
  f.finish(); await opening;
  assert.equal(f.closed(), 0);
});

test("current opener error remains visible and permits retry", async () => {
  const f = fixture(), opening = f.controller.floatChat("room@g.us");
  f.fail("window limit reached"); await opening;
  assert.deepEqual(f.controller.state(), { floatBusy: false, floatError: "window limit reached" });
  assert.equal(f.closed(), 0);
});

test("late opener errors cannot overwrite reopened menu", async () => {
  const f = fixture(), opening = f.controller.floatChat("room@g.us");
  f.controller.reopen(); f.fail("old failure"); await opening;
  assert.deepEqual(f.controller.state(), { floatBusy: false, floatError: null });
});
