import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import ts from "typescript";
import { replaceSlashToken, slashToken } from "../lib/utils/slash-commands.ts";

test("actual ComposerBar delayed slash tick cannot restore an old caret after a scope switch", async () => {
  const source = readFileSync(new URL("../lib/composer/ComposerBar.svelte", import.meta.url), "utf8").match(/<script[^>]*>([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("bar.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const method = tree.statements.find((statement) => ts.isFunctionDeclaration(statement) && statement.name?.text === "chooseSlash")!.getText(tree);
  const code = ts.transpileModule(method, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
  for (const field of ["account", "chat", "generation"]) {
    let release!: () => void;
    const waiting = new Promise<void>((resolve) => { release = resolve; });
    const carets: number[] = [], flows: string[] = [];
    const input = { selectionStart: 5, selectionEnd: 5, setSelectionRange: (at: number) => carets.push(at) };
    const bindings = { disabled: false, account: "a", selectedChat: "room@g.us", generation: 1, composerInput: input,
      draft: "/poll", dismissedSlash: null, slashKey: "captured", pickerTab: null, replaceSlashToken,
      oncreatekind: (kind: string) => flows.push(kind), onslashcommand: () => {}, tick: () => waiting, updateCaret: () => carets.push(-1) };
    const controller = new Function(...Object.keys(bindings), code + `
      return { chooseSlash, change(field) { if(field==='account')account='b'; else if(field==='chat')selectedChat='other@g.us'; else generation=2; draft='new draft'; }, draft:()=>draft };`
    )(...Object.values(bindings));
    const pending = controller.chooseSlash({ command: "poll", token: slashToken("/poll", 5), account: "a", chat: "room@g.us", generation: 1 });
    assert.deepEqual(flows, ["poll"]);
    controller.change(field); release(); await pending;
    assert.deepEqual(carets, []); assert.equal(controller.draft(), "new draft");
  }
});

test("the existing mention payload encodes slash group @all without expanding everyone", () => {
  const source = readFileSync(new URL("../lib/state/composer.svelte.ts", import.meta.url), "utf8");
  const tree = ts.createSourceFile("composer.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const state = tree.statements.find((statement) => ts.isClassDeclaration(statement) && statement.name?.text === "ComposerState") as ts.ClassDeclaration;
  const method = state.members.find((entry) => entry.name?.getText(tree) === "mentionPayload")!.getText(tree);
  const code = ts.transpileModule(`class State { ${method} }`, { compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext } }).outputText;
  const payload = new Function("members", code + "\nreturn State.prototype.mentionPayload;")({ participants: [{ jid: "member@s.whatsapp.net" }], groupAliases: [] });
  assert.deepEqual(payload.call({ draft: "@all ", chosenMentions: [{ name: "all", jid: "@all" }] }), { text: "@all", jids: ["@all"] });
});
