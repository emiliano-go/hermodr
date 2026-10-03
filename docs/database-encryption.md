# Database encryption

Database encryption is opt-in under Settings → Privacy. The default is off,
preserving existing installations. The choice takes effect at the next cold
start, before account services open their databases. Existing plaintext files
are exported into SQLCipher databases, validated, then replaced. A failed
migration leaves the affected account unavailable rather than opening a fresh
database or silently falling back to plaintext.

## Coverage

Each account has one independently generated 32-byte random key. Postal stores
it through the OS credential store under `com.postal.database`, with the owner
`account/<account-id>`. Keys are not stored in settings, database paths or logs.
The credential store must be accessible when starting an encrypted account.
An OS file lock in Postal's private data directory serializes key creation and
readback across processes, including concurrent development launches.
The Windows credential store, macOS Keychain and the configured Linux secret
service are provided by the existing keyring integration.

Encryption covers the message archive, including names, poll/event secrets,
receipts and media paths; aliases; favorite chats; scheduled messages; and both
the main and Android companion protocol-session databases. Startup also checks
recognized older session files and the previous history location. Session
writers and readers receive the key before SQLite pragmas or SDK migrations.
SQLCipher also protects database WAL pages.

Downloaded attachments, account metadata, device settings, logs and exported conversation or
backup files remain plaintext. Exported backups intentionally contain portable
message data and aliases, not protocol-session credentials. Restoring a backup
while encryption is active writes the new account databases encrypted from
their first write. A backup alone does not include its source account's key.

## Failure and recovery

Turning the preference off stops opting new accounts into encryption after a
restart. It does not decrypt existing databases or make their key optional.
Postal does not replace a missing, corrupt or inaccessible existing key. Losing
the key prevents opening its encrypted databases; there is no password or
transport-session substitute. Preserve OS credential-store access along with
any encrypted database copies. Removing an account does not currently remove
its credential-store entry.

Migration uses sibling staging and recovery files with a verified ownership
ledger. It checkpoints committed WAL data, checks schema and row contents,
preserves SQLite metadata, and verifies the encrypted replacement before
removing the original plaintext backup. Startup resumes an interrupted owned
migration. Foreign artifacts, changed source data, wrong keys, symlinks and
uncheckpointed busy databases cause an error. Do not manually delete recovery
artifacts while a migration is incomplete; the original file may still be
needed. Successful cleanup removes Postal's plaintext migration copy, but
cannot erase historical filesystem snapshots or previously exported backups.

## Building

Postal links SQLCipher through `rusqlite` with vendored OpenSSL enabled. The
SDK's SQLite provider uses that same SQLite library. Normal commands use
`scripts/cargo.mjs` and `scripts/tauri.mjs`; `pnpm tauri`, `pnpm generate:wire`
and `pnpm check:wire` already use these wrappers.

On Windows, the Cargo wrapper recognizes a complete existing OpenSSL SDK in
`C:/Program Files/OpenSSL-Win64` or `C:/OpenSSL-Win64`, including its headers and
matching MSVC static MD libraries. It applies the documented `OPENSSL_*`
variables only to that child process. Explicit provider paths remain honored.
Without such an SDK, the vendored build needs Perl with OpenSSL's required
modules and the usual native build tools. The wrapper does not install tools
or change global environment variables. Bare `cargo` does not receive its
provider detection.

Tests exercise synthetic databases and fake key providers. They do not prove
availability of an OS credential service on a particular installed desktop.
