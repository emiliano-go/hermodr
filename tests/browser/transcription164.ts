import { mount } from "svelte";
import type { StoredTranscript, TranscriptionEvent, TranscriptionView, PluginInfo } from "../../src/lib/utils/wire";

const plugin = { id: "org.postal.test-stt", name: "Synthetic STT", version: "1", api_version: 1, entrypoint: "synthetic",
  activation: "lazy", idle_timeout_secs: null, capabilities: ["transcribe"], enabled: false, state: "idle", error: null,
  contributes: { commands: [], transcription: { id: "stt", providers: [
    { id: "local-whisper", name: "Synthetic local", kind: "local", transmits_audio: false, requires_key: false },
    { id: "openai", name: "Synthetic cloud", kind: "cloud", transmits_audio: true, requires_key: true },
  ] } } } as PluginInfo;
export const fixture = {
  view: { settings: { plugin_id: plugin.id, provider: "local-whisper", whisper_executable: null, decoder_executable: null,
    model: "tiny.bin", model_sha256: "0".repeat(64), language: null, idle_timeout_secs: null }, plugins: [plugin], cloud_consents: [],
    key_configured: false, data_directory: "SYNTHETIC-DATA-NO-FILES", errors: [], failures: [] } as TranscriptionView,
  hold: false, failure: false, calls: [] as { command: string; args: unknown }[],
  cache: new Map<string, StoredTranscript>(), overrides: new Map<string, boolean | null>(),
  pending: new Map<string, { finish: () => void; cancel: () => void }>(),
};
const callbacks = new Map<number, (event: unknown) => void>();
const listeners = new Map<number, { event: string; handler: number }>();
let next = 0;
function emit(event: string, payload: unknown) {
  for (const [id, listener] of listeners) if (listener.event === event) callbacks.get(listener.handler)?.({ event, id, payload });
}
function target(args: Record<string, unknown>) { return JSON.stringify([args.accountId, args.chat, args.id]); }
function event(args: Record<string, unknown>, status: string, transcript: StoredTranscript | null = null, error: string | null = null) {
  emit("transcription-event", { account_id: args.accountId, chat: args.chat, id: args.id, status, transcript, error } as TranscriptionEvent);
}
export async function invoke<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  fixture.calls.push({ command, args: JSON.parse(JSON.stringify(args)) });
  let result: unknown;
  if (command === "transcription_settings") result = structuredClone(fixture.view);
  else if (command === "set_transcription_settings") { fixture.view.settings = JSON.parse(JSON.stringify(args.settings)) as TranscriptionView["settings"]; emit("transcription-settings-changed", null); }
  else if (command === "set_plugin_enabled") { plugin.enabled = Boolean(args.enabled); emit("transcription-settings-changed", null); }
  else if (command === "grant_transcription_cloud_consent") {
    fixture.view.cloud_consents = args.approved ? [{ plugin_id: String(args.pluginId), provider: String(args.providerId) }] : [];
  } else if (command === "configure_transcription_key") fixture.view.key_configured = true;
  else if (command === "forget_transcription_key") fixture.view.key_configured = false;
  else if (command === "install_transcription_model") { if (fixture.failure) throw new Error("synthetic model failure"); }
  else if (command === "chat_auto_transcribe") result = fixture.overrides.get(JSON.stringify([args.accountId, args.chat])) ?? null;
  else if (command === "set_chat_auto_transcribe") fixture.overrides.set(JSON.stringify([args.accountId, args.chat]), args.enabled as boolean | null);
  else if (command === "message_transcript") result = fixture.cache.get(target(args)) ?? null;
  else if (command === "cancel_transcription") fixture.pending.get(target(args))?.cancel();
  else if (command === "transcribe_message") {
    if (!plugin.enabled) throw new Error("synthetic plugin disabled");
    if (fixture.view.settings.provider === "openai" && !fixture.view.cloud_consents.length) throw new Error("synthetic cloud consent required");
    if (fixture.view.settings.provider === "openai" && !fixture.view.key_configured) throw new Error("synthetic cloud key required");
    event(args, "started");
    result = await new Promise<StoredTranscript>((resolve, reject) => {
      const finish = () => {
        fixture.pending.delete(target(args));
        if (fixture.failure) { event(args, "failed", null, "synthetic provider failure"); reject(new Error("synthetic provider failure")); return; }
        const transcript: StoredTranscript = { chat: String(args.chat), id: String(args.id), text: `Synthetic transcript for ${args.accountId}`,
          language: "en", provider: fixture.view.settings.provider, created_at: 1 };
        fixture.cache.set(target(args), transcript); event(args, "completed", transcript); resolve(transcript);
      };
      fixture.pending.set(target(args), { finish, cancel: () => { fixture.pending.delete(target(args)); event(args, "cancelled", null, "synthetic cancellation"); reject(new Error("synthetic cancellation")); } });
      if (!fixture.hold) finish();
    });
  } else if (command === "plugin:event|listen") { const id = ++next; listeners.set(id, { event: String(args.event), handler: Number(args.handler) }); result = id; }
  else if (command === "plugin:event|unlisten") listeners.delete(Number(args.eventId));
  else throw new Error(`No synthetic transcription response for ${command}`);
  return result as T;
}

Object.defineProperty(window, "__TAURI_INTERNALS__", { configurable: true, value: {
  invoke, transformCallback: (callback: (event: unknown) => void) => { const id = ++next; callbacks.set(id, callback); return id; },
  unregisterCallback: (id: number) => callbacks.delete(id),
} });
void import("./Transcription164.svelte").then(({ default: Fixture }) => mount(Fixture, { target: document.getElementById("app")! }));
