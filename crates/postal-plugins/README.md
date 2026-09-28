# Read-only sidecar plugins

Postal discovers `$APPDATA/plugins/<directory>/plugin.json` at startup (the exact
platform directory appears in Settings → Plugins). Restart after installing or
changing a manifest. New plugins are disabled. Enabling requires an explicit
`events:read` grant, persisted in the app configuration's `plugin-grants.json`.
Grants apply to all connected accounts. Disabling stops the process.

Plugins are native executables, not webview scripts. Only install code you trust.
`events:read` exposes message content, details, and service events; pairing QR codes
are excluded. Host methods, command contributions and plugin UI events are not
available in v1. These checks constrain the host API, **not the operating system**:
plugins can read files and access networks with the user's permissions. There is
no OS sandbox or installer signature verification in this crate.

Build the sample with `cargo build -p postal-plugins --example sample`, copy the
executable into a new plugin directory, and add this manifest (`sample.exe` on
Windows):

```json
{
  "id": "com.example.sample",
  "name": "Sample Plugin",
  "version": "0.1.0",
  "api_version": 1,
  "entrypoint": "sample",
  "activation": "lazy",
  "idle_timeout_secs": 30,
  "capabilities": ["events:read"],
  "contributes": { "commands": [] }
}
```

Entrypoints must stay inside the plugin directory after canonicalization. Absolute
paths, parent components, symlink escapes, duplicate IDs, unknown fields or
capabilities, nonempty command contributions, and unsupported API versions fail
validation. Idle timeouts are null (immediate) or 1–86400 seconds.

The host clears the environment (Windows retains only SystemRoot and WINDIR), sets
the working directory to the plugin directory, pipes stdio, and creates no Windows
console window. Resolve plugin resources relative to the working directory; do
not depend on inherited PATH, HOME, credentials or application environment.

## JSON lines protocol

1. Host sends `{"type":"hello","api_version":1,"capabilities":["events:read"]}`.
2. Plugin returns `{"type":"ready","name":"Sample Plugin"}` within 5 seconds.
3. Host sends `{"type":"event","seq":12,"event":{"kind":"connected"}}`.
4. Plugin completes processing and returns `{"type":"ack","seq":12}` within
   10 seconds. Sequence numbers are process-independent within an app run. Each
   plugin gets events in order, one unacknowledged event at a time.
5. Host sends `{"type":"shutdown"}` on disable, app exit or lazy idle expiry.
   Plugin must exit within 2 seconds; otherwise the host kills and reaps it.

`{"type":"log","level":"info","message":"..."}` is logged with the plugin ID.
Stderr is logged separately. `call` messages receive an error; no host methods are
implemented. Malformed, unknown and oversized lines are dropped and logged. The
newline buffer is capped at 1 MiB and the stdout queue at 16 lines. Host events
larger than 1 MiB are refused. Each plugin has 64 queued events; saturation drops
new events and exposes an error in Settings. There is no durable event replay.

Eager plugins start when enabled and stay resident. Three failures per enable/app
run disable the plugin persistently, with 250ms then 500ms restart backoff. A lazy
plugin starts on its next event, queues events during its handshake, and drains
queued work before unloading. Idle time begins after a matching acknowledgement;
unsolicited replies do not extend it. Lazy failures surface an error and retry
only when another event arrives. Events whose delivery failed are not replayed.

`PluginHost::send_event` is the single activation-and-acknowledgement path for
callers needing completion; `publish` uses the same supervisor without waiting
and cannot block the core service on a slow plugin. The core crate has no plugin
dependency. Filesystem discovery is startup-only; manifests are checked again
before each spawn. Replace executables only while disabled.

## Verification

`cargo test -p postal-plugins` compiles a synthetic Rust child executable and tests
temporary plugin directories, consent persistence, handshakes, acknowledgements,
malformed/oversized output, rejected calls, lazy queuing/idle unload, crashes,
bounded queues, environment clearing and graceful/forced shutdown. It does not
load installed plugins or connect to WhatsApp. CI runs this suite on Windows and
Linux; symlink escape coverage is Unix-specific.
