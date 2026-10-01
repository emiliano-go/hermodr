import assert from "node:assert/strict";
import test from "node:test";
import { get } from "svelte/store";
import { checkedLibrary, clipMetadata, clipName, MAX_SOUND_BYTES, soundFile, soundRecords } from "./soundboard/library.ts";
import type { SoundRecord, SoundRepository } from "./soundboard/library.ts";
import { createSoundboard } from "./soundboard/state.ts";

const stateGlobal = globalThis as unknown as { $state?: (value: unknown) => unknown };
const originalState = stateGlobal.$state;
stateGlobal.$state = (value) => value;
const shortcuts = await import("./soundboard/shortcuts.ts");
const { keybinds } = await import("./utils/keybinds.svelte.ts");
if (originalState) stateGlobal.$state = originalState; else delete stateGlobal.$state;

function wav(size = 48, name = "ding.wav", type = "audio/wav") { return new File([new Uint8Array(size)], name, { type }); }
function record(id: string, size = 48, shortcut: number | null = null) { return { ...soundFile(wav(size), id, shortcut), id }; }
function repository(initial: SoundRecord[] = []) {
  let records = initial;
  let failed = false;
  const value: SoundRepository = {
    list: async () => records.map(clipMetadata),
    read: async (id) => { const row = records.find((row) => row.id === id); if (!row) throw new Error("missing clip"); return row; },
    save: async (row) => { if (failed) throw new Error("quota"); records = checkedLibrary(records, row); return records.map(clipMetadata); },
    remove: async (id) => { if (failed) throw new Error("quota"); records = records.filter((row) => row.id !== id); return records.map(clipMetadata); },
  };
  return { value, fail(next: boolean) { failed = next; }, rows: () => records };
}

test("soundboard audio types stay aligned with attachment classification and bounded metadata", () => {
  for (const extension of ["ogg", "opus", "mp3", "m4a", "aac", "wav"]) assert.ok(soundFile(wav(48, `clip.${extension}`, ""), "Clip").type.startsWith("audio/"));
  for (const name of ["clip.webm", "clip.mp4", "clip.txt", "wav", ".wav"]) assert.throws(() => soundFile(wav(48, name), "Clip"), /Choose an OGG/);
  assert.throws(() => soundFile(wav(48, "clip.wav", "image/png"), "Clip"), /audio file/);
  assert.throws(() => soundFile(wav(0), "Clip"), /smaller than 8 MiB/);
  assert.throws(() => soundFile(wav(MAX_SOUND_BYTES + 1), "Clip"), /smaller than 8 MiB/);
  assert.equal(clipName("  Friendly   sound  "), "Friendly sound");
  assert.throws(() => clipName(" "), /1–64/); assert.throws(() => clipName("x".repeat(65)), /1–64/);
  const safe = soundFile(wav(48, "folder\\ding.wav"), "Ding");
  assert.equal(safe.filename, "ding.wav"); assert.equal("blob" in clipMetadata(safe), false);
  assert.throws(() => soundRecords([{ ...safe, size: 123 }]), /invalid clip/);
});

test("soundboard capacity and shortcut ownership remain bounded when a clip is replaced", () => {
  const full = Array.from({ length: 24 }, (_, index) => record(String(index)));
  assert.throws(() => checkedLibrary(full, record("new")), /24 clips/);
  assert.equal(checkedLibrary(full, { ...full[0], name: "Renamed" }).length, 24);
  const large = Array.from({ length: 4 }, (_, index) => record(String(index), MAX_SOUND_BYTES));
  assert.throws(() => checkedLibrary(large, record("new")), /32 MiB/);
  const bound = [record("one", 48, 1)];
  assert.throws(() => checkedLibrary(bound, record("other", 48, 1)), /another clip/);
  assert.equal(checkedLibrary(bound, { ...bound[0], name: "Changed" })[0].shortcut, 1);
});

test("soundboard changes report durability failures and retain saved clips for retry", async () => {
  const repo = repository([record("existing")]), state = createSoundboard(repo.value);
  await state.load(); repo.fail(true);
  assert.equal(await state.add(wav(), "Failed", null), false);
  assert.equal(get(state).clips[0].id, "existing"); assert.match(get(state).error!, /quota/);
  assert.equal(await state.remove("existing"), false); assert.equal(repo.rows().length, 1);
  repo.fail(false);
  assert.equal(await state.update("existing", "Renamed", 2), true);
  assert.equal(get(state).clips[0].name, "Renamed"); assert.equal(get(state).clips[0].shortcut, 2);
  assert.equal(get(state).error, null);
  assert.equal(await state.remove("existing"), true); assert.deepEqual(get(state).clips, []);
});

test("a late metadata read cannot undo a successful library change", async () => {
  const repo = repository(); let finish!: (rows: ReturnType<typeof clipMetadata>[]) => void;
  repo.value.list = () => new Promise((resolve) => { finish = resolve; });
  const state = createSoundboard(repo.value), loading = state.load();
  assert.equal(await state.add(wav(), "Saved", null), true);
  finish([]); await loading;
  assert.equal(get(state).clips[0].name, "Saved"); assert.equal(get(state).loading, false); assert.equal(get(state).busy, false);
});

test("clip sending checks the captured scope after the blob read and leaves caller drafts untouched", async () => {
  const repo = repository([record("saved")]); let finish!: (record: SoundRecord) => void;
  repo.value.read = () => new Promise((resolve) => { finish = resolve; });
  const state = createSoundboard(repo.value), scope = { account: "a", chat: "room@g.us", generation: 3 };
  let current = true, sent = 0;
  const stale = state.send("saved", scope, () => current, async () => { sent++; });
  current = false; finish(repo.rows()[0]); await assert.rejects(stale, /account or chat changed/); assert.equal(sent, 0);
  const draft = "unrelated text", pending = [wav(48, "unrelated.wav")];
  current = true;
  const send = state.send("saved", scope, () => current, async (file, target) => {
    assert.deepEqual(target, scope); assert.equal(file.name, "ding.wav"); assert.equal(file.type, "audio/wav");
    assert.deepEqual(new Uint8Array(await file.arrayBuffer()), new Uint8Array(48)); sent++;
  });
  finish(repo.rows()[0]); await send;
  assert.equal(sent, 1); assert.equal(draft, "unrelated text"); assert.equal(pending[0].name, "unrelated.wav");
});

test("optional clip shortcuts reuse exact keybind matching and reject existing or changed conflicts", () => {
  const clips = [clipMetadata(record("one", 48, 1))];
  const event = { key: "1", ctrlKey: true, altKey: true, metaKey: false, shiftKey: false, repeat: false, isComposing: false, defaultPrevented: false } as KeyboardEvent;
  assert.equal(shortcuts.soundShortcutLabel(1), "Ctrl+Alt+1");
  assert.equal(shortcuts.shortcutClip(event, clips)?.id, "one");
  assert.equal(shortcuts.shortcutClip({ ...event, shiftKey: true } as KeyboardEvent, clips), null);
  assert.equal(shortcuts.shortcutClip({ ...event, repeat: true } as KeyboardEvent, clips), null);
  assert.equal(shortcuts.shortcutClip({ ...event, keyCode: 229 } as KeyboardEvent, clips), null);
  assert.equal(shortcuts.shortcutClip({ ...event, getModifierState: () => true } as KeyboardEvent, clips), null);
  assert.match(shortcuts.soundShortcutConflict(1, [...clips, clipMetadata(record("other", 48, 1))], "one")!, /another clip/);
  const original = keybinds.editLast;
  try {
    keybinds.editLast = shortcuts.soundBinding(1)!;
    assert.match(shortcuts.soundShortcutConflict(1, clips, "one")!, /edit last/);
    assert.equal(shortcuts.shortcutClip(event, clips), null);
  } finally { keybinds.editLast = original; }
  assert.equal(shortcuts.soundBinding(0), null); assert.equal(shortcuts.soundBinding(10), null);
});
