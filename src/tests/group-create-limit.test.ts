import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";

test("group picker allows server-supported sizes above the old 256-person cap", () => {
  const source = readFileSync(new URL("../lib/chat/NewGroup.svelte", import.meta.url), "utf8").match(/<script[^>]*>([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile("group.ts", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const toggle = tree.statements.find((node) => ts.isFunctionDeclaration(node) && node.name?.text === "toggle");
  const valid = tree.statements.filter(ts.isVariableStatement).flatMap((node) => node.declarationList.declarations)
    .find((node) => node.name.getText(tree) === "valid")?.initializer;
  assert.ok(toggle && valid && ts.isCallExpression(valid));
  const context = { busy: false, created: null, chosen: {} as Record<string, string>, subjectLength: 1,
    current: () => true, members: { displayName: (name: string) => name },
    get picked() { return Object.keys(this.chosen); },
  };
  const actions = runInNewContext(ts.transpileModule(`${toggle.getText(tree)}\n({toggle, valid: () => (${valid.arguments[0].getText(tree)})})`,
    { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context);
  for (let index = 0; index < 300; index++) actions.toggle({ jid: `${index}@s.whatsapp.net`, name: `Contact ${index}` });
  assert.equal(context.picked.length, 300);
  assert.equal(actions.valid(), true);
  actions.toggle({ jid: "299@s.whatsapp.net", name: "Contact 299" });
  assert.equal(context.picked.length, 299);
  context.busy = true;
  actions.toggle({ jid: "300@s.whatsapp.net", name: "Contact 300" });
  assert.equal(context.picked.length, 299);
});
