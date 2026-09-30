Rust Serde DTOs define IPC, plugin events and archive JSON. Fields use Rust
names and explicit Serde renames. Existing camelCase event `kind` values and
`Target.fromMe` stay compatible. Tauri command argument names use camelCase;
nested DTO fields follow Serde.

`src/lib/utils/wire.ts` is derived from Rust with `ts-rs`; import these types
instead of copying them. Flattened fields, skipped internal fields and tagged
enums come from Serde metadata. JSON integers map to TypeScript `number`.
Run `pnpm generate:wire` after DTO changes and `pnpm check:wire` to detect drift.
New DTOs need a `wire-types` gated `ts_rs::TS` derive. Register a new command
result or input in its crate's `visit_wire_types`; nested types are discovered
from compiler metadata.
Generation uses the optional `wire-types` Cargo feature. Normal builds do not
include the generator dependency.
The Tauri CI matrix enables this feature to check the committed bindings. On
Windows, generation needs the same MSVC developer environment as other Cargo
commands.

The v1 plugin handshake, event envelope and reply types are generated too.
`HostMessage<ServiceEvent>` describes its event payload; pairing QR events are
excluded by the host before delivery. Archive manifests retain format versions
and snake_case keys. Backup table rows are raw database columns; they are not
renamed by the wire generator. Existing archives and settings keep their
current serialized representation. A future field rename requires an explicit
format migration and consumer compatibility handling.
Rust keeps its input validation, aliases and legacy retention readers. The
generated TypeScript contract describes the current serialized output.
