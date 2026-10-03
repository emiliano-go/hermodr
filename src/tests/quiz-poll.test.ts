import assert from "node:assert/strict";
import test from "node:test";
import { readFileSync } from "node:fs";
import { runInNewContext } from "node:vm";
import ts from "typescript";
import { prepareQuiz, quizFeedback, quizScopeMatches, type QuizScope } from "../lib/utils/quiz-poll.ts";
import { LocalizedError, normalizeError } from "../lib/i18n/errors.ts";
import { t } from "../lib/i18n/localizer.ts";

function functions(file: string, context: Record<string, any>) {
  Object.assign(context, { normalizeError, t });
  Object.defineProperty(context, "liveScope", { get: () => context.scope ?? null });
  const source = readFileSync(new URL(file, import.meta.url), "utf8").match(/<script lang="ts">([\s\S]*?)<\/script>/)![1];
  const tree = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, ts.ScriptKind.TS);
  const body = tree.statements.filter(ts.isFunctionDeclaration).map((item) => item.getText(tree)).join("\n");
  runInNewContext(ts.transpileModule(body, { compilerOptions: { target: ts.ScriptTarget.ES2022 } }).outputText, context);
  return context;
}
const scope = (): QuizScope => ({ account: "alpha", chat: "group@g.us", generation: 1, requestKey: 1 });
const rows = [{ id: 0, text: " Alpha " }, { id: 1, text: " " }, { id: 2, text: " Beta " }];

test("quiz answer tracks stable row through trimming, blanks, edits and removals", () => {
  assert.deepEqual(prepareQuiz(" Question ", rows, 2), { question: "Question", options: ["Alpha", "Beta"], correctIndex: 1, error: null });
  assert.equal(prepareQuiz("Question", rows.filter((row) => row.id !== 1), 2).correctIndex, 1);
  const changed = [{ id: 2, text: "Renamed Beta" }, { id: 3, text: "Gamma" }];
  assert.equal(prepareQuiz("Question", changed, 2).correctIndex, 0);
  assert.equal(prepareQuiz("Question", [{ ...changed[0], text: " " }, changed[1], { id: 4, text: "Delta" }], 2).correctIndex, null);
  const context = functions("../lib/chat/CreateDialog.svelte", { options: [...rows], correctRow: 2 });
  context.removeOption(0);
  assert.equal(context.correctRow, 2);
  context.removeOption(2);
  assert.equal(context.correctRow, null);
});

test("quiz form refuses missing correct answer, duplicate or invalid option counts", () => {
  assert.ok(prepareQuiz("", rows, 2).error);
  assert.ok(prepareQuiz("Question", rows, null).error);
  assert.ok(prepareQuiz("Question", [{ id: 0, text: "A" }], 0).error);
  assert.ok(prepareQuiz("Question", [{ id: 0, text: " A " }, { id: 1, text: "A" }], 0).error);
  assert.ok(prepareQuiz("Question", Array.from({ length: 13 }, (_, id) => ({ id, text: String(id) })), 0).error);
  assert.equal(prepareQuiz("Question", Array.from({ length: 12 }, (_, id) => ({ id, text: String(id) })), 11).error, null);
});

test("quiz feedback uses native correctness only, leaving unknown result explicit", () => {
  assert.equal(quizFeedback(true), "Your answer is correct.");
  assert.equal(quizFeedback(false), "Your answer is incorrect.");
  assert.equal(quizFeedback(null), "Answer result unavailable.");
  assert.equal(quizFeedback(undefined), "Answer result unavailable.");
});

test("creator captures quiz payload and ignores stale submit close/error/finally", async () => {
  for (const field of ["account", "chat", "generation", "requestKey"] as const) {
    let release!: () => void;
    let closed = 0;
    const context = functions("../lib/chat/CreateDialog.svelte", { kind: "poll", scope: scope(), generation: 1, valid: true, busy: false, failed: null,
      quiz: true, quizDraft: prepareQuiz("Question", rows, 2), quizScopeMatches,
      onclose: () => closed++, oncreate: (value: unknown) => {
        assert.deepEqual(JSON.parse(JSON.stringify(value)), { question: "Question", options: ["Alpha", "Beta"], correctIndex: 1 });
        return new Promise<void>((yes) => { release = yes; });
      } });
    const pending = context.submit();
    context.scope[field] = field === "account" || field === "chat" ? "next" : 2;
    context.busy = true;
    context.failed = "New dialog error";
    release();
    await pending;
    assert.equal(closed, 0);
    assert.equal(context.failed, "New dialog error");
    assert.equal(context.busy, true);
  }
});

test("creator shows rejection and retains quiz draft; ordinary poll payload stays compatible", async () => {
  const context = functions("../lib/chat/CreateDialog.svelte", { kind: "poll", scope: scope(), generation: 1, valid: true, busy: false, failed: null,
    quiz: true, quizDraft: prepareQuiz("Question", rows, 2), quizScopeMatches, onclose: () => assert.fail("Rejected create must remain open"),
    oncreate: async () => { throw new Error("Synthetic quiz refusal"); } });
  await context.submit();
  assert.ok(context.failed instanceof LocalizedError);
  assert.equal(context.failed.code, "error.operation_failed");
  assert.equal(context.failed.message, t("error.operation_failed"));
  assert.match(context.failed.diagnostic ?? "", /Synthetic quiz refusal/);
  assert.equal(context.quizDraft.correctIndex, 1);
  assert.equal(context.busy, false);
  context.quiz = false; context.question = "Question"; context.filled = ["Alpha", "Beta"]; context.multi = true;
  context.oncreate = async (value: unknown) => assert.deepEqual(JSON.parse(JSON.stringify(value)), { question: "Question", options: ["Alpha", "Beta"], multi: true });
  context.onclose = () => {};
  await context.submit();
  assert.equal(context.failed, null);
});

test("quiz vote rejects visibly without optimism and stale vote cannot alter another scope", async () => {
  const poll = { id: "quiz", multi: false, quiz: { my_correct: null } };
  const context = functions("../lib/messages/cards/PollCard.svelte", { poll, scope: scope(), generation: 1, canVote: true, multi: false, mine: [], busy: false,
    failed: "", quizScopeMatches, onvote: async () => { throw new Error("Synthetic vote refusal"); } });
  await context.toggle("Beta");
  assert.ok(context.failed instanceof LocalizedError);
  assert.equal(context.failed.code, "error.operation_failed");
  assert.equal(context.failed.message, t("error.operation_failed"));
  assert.match(context.failed.diagnostic ?? "", /Synthetic vote refusal/);
  assert.equal(context.poll.quiz.my_correct, null);
  let reject!: (error: Error) => void;
  context.onvote = () => new Promise((_, no) => { reject = no; });
  const pending = context.toggle("Beta");
  context.scope.account = "beta";
  context.failed = "New scope error";
  reject(new Error("Old scope error"));
  await pending;
  assert.equal(context.failed, "New scope error");
  assert.equal(context.busy, true);
  assert.equal(context.voterName("@me"), "You");
});

test("unmounted creator cannot show late rejection or close replacement dialog", async () => {
  let reject!: (error: Error) => void;
  let closed = 0;
  const context = functions("../lib/chat/CreateDialog.svelte", { kind: "poll", scope: scope(), generation: 1, valid: true, busy: false, failed: null,
    quiz: true, quizDraft: prepareQuiz("Question", rows, 2), quizScopeMatches, onclose: () => closed++,
    oncreate: () => new Promise((_, no) => { reject = no; }) });
  const pending = context.submit();
  context.generation++;
  context.failed = "Replacement dialog error";
  reject(new Error("Old dialog rejection"));
  await pending;
  assert.equal(context.failed, "Replacement dialog error");
  assert.equal(closed, 0);
});

test("native vote eligibility blocks quiz send and successful callback never invents correctness", async () => {
  let sent = 0;
  const context = functions("../lib/messages/cards/PollCard.svelte", { poll: { id: "quiz", quiz: { my_correct: null } }, scope: scope(), generation: 1,
    canVote: false, multi: false, mine: [], busy: false, failed: "", quizScopeMatches, onvote: async () => sent++ });
  await context.toggle("Beta");
  assert.equal(sent, 0);
  context.canVote = true;
  await context.toggle("Beta");
  assert.equal(sent, 1);
  assert.equal(context.poll.quiz.my_correct, null);
  assert.equal(context.busy, false);
});
