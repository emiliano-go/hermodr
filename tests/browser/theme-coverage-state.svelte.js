import { untrack } from "svelte";
import { messages } from "$lib/utils/theme-preview";

export const fixture = $state({ mode: "ready", calls: /** @type {string[]} */ ([]) });
export const session = $state({ activeAccount: "theme-fixture" });
export const transcription = { configure() {} };
/** @param {string} path */
export const convertFileSrc = (path) => path;
export const listen = async () => () => {};

/** @param {string} command */
export async function invoke(command) {
  const mode = untrack(() => { fixture.calls.push(command); return fixture.mode; });
  if (mode === "error") throw new Error("Synthetic fixture error");
  if (command === "message_page") return { messages: mode === "empty" ? [] : [
    { ...messages.find((message) => !message.from_me && message.media_kind === null), chat: "synthetic@g.us", text: "Synthetic preview body" },
  ], has_more: false };
  if (command === "boolean_props") return (mode === "empty" ? [] : [
    { name: "synthetic_enabled", code: 1, default: false, value: true },
    { name: "synthetic_disabled", code: 2, default: true, value: false },
  ]);
  if (command === "transcription_settings") return {
    settings: { plugin_id: mode === "empty" ? null : "synthetic", provider: "local", model: "fixture.bin",
      model_sha256: null, decoder_executable: null, whisper_executable: null, language: null, idle_timeout_secs: null },
    plugins: mode === "empty" ? [] : [{ id: "synthetic", name: "Synthetic provider", enabled: false,
      contributes: { transcription: { providers: [{ id: "local", name: "Synthetic local", kind: "local",
        transmits_audio: false, requires_key: false }] } } }],
    cloud_consents: [], key_configured: false, data_directory: null, errors: [],
  };
  throw new Error(`Unexpected synthetic operation: ${command}`);
}
