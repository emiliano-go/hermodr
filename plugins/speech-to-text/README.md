# Speech to text sidecar

`postal-stt` is a native JSONL sidecar for Postal's `transcribe` capability. The
host starts it only for a transcription or an explicit model download, then
unloads it after completed work. It receives selected audio bytes, never a
message-store path or database connection. Local Whisper is the default provider
when this plugin is selected. OpenAI and Deepgram require provider-specific
consent and a key from the host's operating-system credential store.

## Build and install

```powershell
cargo build --release --manifest-path plugins/speech-to-text/Cargo.toml
```

Copy `target/release/postal-stt.exe` on Windows, or `target/release/postal-stt` on
Unix, from this package into a trusted plugin directory. Generate the manifest
with the executable's `--manifest` option; it emits the correct platform
entrypoint. Install and verify the package through Postal's trusted plugin
installation workflow, then restart Postal. Nothing starts merely because the
plugin is discovered or enabled.

Settings → Speech to text selects this plugin and provider and requests the
`transcribe` grant. It does not grant `events:read`. Set the audio decoder to an
absolute path to a user-installed FFmpeg executable. Set Local Whisper's engine
to an absolute path to user-installed `whisper-cli`, with its dependencies next
to that executable. Configure a model filename and a trusted SHA-256 in the
plugin's own `data` directory. The model must stay inside that directory after
canonicalization. Tiny multilingual is the intended low-memory model.

Models and engines are not included in this package. The plugin itself is MIT.
Whisper.cpp is MIT. FFmpeg remains user-installed and is neither bundled nor
linked, since its licensing differs from Postal's MIT/Apache-2.0 shipment rule.
Sherpa and Vosk are optional future engines and are not advertised as working
providers by this package. Moonshine and Cactus are not shipped.

An explicit **Download and verify model** action uses the configured HTTPS URL
and SHA-256. Downloads have a 256 MiB limit, refuse redirects, verify the entire
file, and publish it without replacing an existing model. A failed or cancelled
download leaves the existing model intact. The equivalent standalone operation
is:

```text
postal-stt --install-model HTTPS_URL SHA256 DATA_DIRECTORY FILENAME
```

There is no model download during startup, plugin enable, or transcription.

## Providers and audio

The host passes OGG/Opus or PCM WAV bytes as base64. Local Whisper and OpenAI use
the configured decoder to produce mono 16 kHz PCM16 WAV. The decoder has a file
size and duration limit and runs without a shell, inherited stdin or application
environment. Local Whisper then runs `whisper-cli -ng -oj`; the plugin parses its
JSON segments and detected language. All temporary audio and engine output stay
under the plugin's data directory and are removed after success, failure,
timeout or cancellation.

OpenAI receives multipart WAV through its transcription endpoint with
`whisper-1` and `verbose_json`. Deepgram receives the original bytes through
`/v1/listen`, with `nova-3`, smart formatting, and language detection or an
explicit language. The endpoints are fixed in production, HTTPS verification
stays enabled, redirects are refused, and application proxy variables are not
inherited. Provider errors report a status or a generic failure, never response
bodies, keys or audio. These are paid services; selecting a cloud provider and
granting consent authorizes audio transmission and may incur charges.

Request audio is capped at 700 KiB to fit the host's 1 MiB JSONL limit. Duration
is capped at ten minutes and decoded WAV at 20 MiB. Transcripts are capped at
128 KiB. Unknown providers, malformed base64, mismatched MIME and audio magic,
missing consent, missing keys, bad model hashes and oversized responses fail
visibly. Empty speech is a valid empty transcript; missing provider data is an
error.

Provider implementations follow the official
[Whisper CLI source](https://github.com/ggml-org/whisper.cpp/blob/master/examples/cli/cli.cpp),
[OpenAI speech-to-text documentation](https://developers.openai.com/api/docs/guides/speech-to-text),
and [Deepgram prerecorded audio documentation](https://developers.deepgram.com/docs/pre-recorded-audio).

## Credentials and host integration

The host stores keys under the keyring service `org.postal.transcription`, with
the user identifier `<plugin-id>/<provider-id>`. Secret Service, Apple Keychain
and Windows Credential Manager are the production backends. There is no
plaintext fallback. A native `postal-transcription-key` helper reads passwords
without echo; no key input or key getter exists in the webview. Package the helper
beside Postal or in its resource directory. The Settings action opens a native
console on Windows, Terminal on macOS, or a terminal emulator on Linux.

The host persists provider-specific consent separately from its ordinary plugin
grant. Revoking consent, changing provider configuration, removing a key,
switching account, disconnecting or cancelling a request stops active work.
The plugin gets a key only in the host's transcription request. Neither the
plugin nor host writes keys to the transcription settings file.

Transcripts are stored by `(chat,id)` with provider, language and creation time.
Re-transcribe replaces that row. Message deletion, revocation and retention
remove its transcript through SQLite triggers. Chat address reconciliation moves
transcripts with messages. Automatic transcription defaults off, follows the
nullable per-chat override, waits for already-downloaded audio, and skips
concealed spoilers and view-once notes. Cloud auto-transcription uses the same
consent and credential gates as manual transcription.

## Protocol

The API version and `hello`/`ready`/`shutdown` handshake remain v1. This capability
uses correlated request/reply messages separately from the event/ack stream:

```json
{"type":"hello","api_version":1,"capabilities":["transcribe"]}
{"type":"ready","name":"Speech to Text"}
{"type":"transcribe","id":5,"provider":"local-whisper","chat":"synthetic@chat","message_id":"ABC","mime":"audio/ogg","duration_ms":3120,"audio":"T2dnUw==","config":{"data_directory":"ABSOLUTE_PLUGIN_DATA","whisper_executable":"ABSOLUTE_ENGINE","decoder_executable":"ABSOLUTE_DECODER","model":"tiny.bin","model_sha256":"TRUSTED_SHA256","cloud_consent":false}}
{"type":"transcript","id":5,"provider":"local-whisper","text":"recognized words","language":"en"}
{"type":"transcribe_error","id":5,"message":"audio engine failed"}
{"type":"cancel","id":5}
{"type":"install_model","id":6,"url":"https://MODEL_URL","sha256":"TRUSTED_SHA256","filename":"tiny.bin","data_directory":"ABSOLUTE_PLUGIN_DATA"}
{"type":"model_installed","id":6,"filename":"tiny.bin"}
{"type":"model_error","id":6,"message":"model checksum mismatch"}
{"type":"shutdown"}
```

The example audio and paths are synthetic. Provider configuration is host-only;
keys are added to `config.api_key` only for the explicitly approved cloud
provider. Request IDs, queue limits, matching replies, cancellation, handshake
timeouts and idle unload belong to the host. Engine children are killed and
reaped before cancelling a transcription. The default request timeout is
300 seconds; model installation has a 600-second host timeout. Streaming
partials remain a later protocol addition.

## Synthetic checks

```powershell
cargo test -p postal-plugins
cargo test --manifest-path plugins/speech-to-text/Cargo.toml --locked
node --experimental-strip-types --test src/tests/transcription.test.ts
```

Tests use generated audio markers and an executable that writes a synthetic WAV
and transcript. Cloud adapter tests bind loopback HTTP only. Native sidecar tests
run the real JSONL process through `PluginHost`; no speech model, cloud account,
provider network, real voice note, WhatsApp database or credential prompt is
opened. Credential tests install keyring's mock backend. These checks establish
protocol and lifecycle behavior, not transcription accuracy or native prompt
behavior on a user's desktop.
