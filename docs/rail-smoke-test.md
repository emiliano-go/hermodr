# Native message rail smoke test

This manual Linux job launches the Tauri shell with a bundled synthetic rail.
It opens a fixture chat, checks that fewer than 30 rows mount from 400 messages,
clicks **Send fixture message**, and waits for its bubble at the bottom.
The fixture replaces IPC and cannot send a WhatsApp message. It checks native
WebKit rendering and rail updates; protocol delivery needs separate tests.

Install the normal Linux build prerequisites, `tauri-driver`, and
`WebKitWebDriver` as described in [Tauri's manual WebDriver setup](https://v2.tauri.app/develop/tests/webdriver/manual-setup/).
Run these commands from the repository root in a Linux desktop session:

```sh
pnpm install --frozen-lockfile
CARGO_TARGET_DIR="$PWD/target/rail-smoke" pnpm tauri build --debug --no-bundle --config tests/browser/rail-smoke/tauri.conf.json
node --experimental-strip-types tests/browser/rail-smoke/check.ts
```

The smoke configuration has its own application identifier and target directory.
The runner only launches `target/rail-smoke/debug/postal`. It also
provides fresh XDG data, configuration, and cache directories to the driver and
its child app. It refuses an occupied WebDriver port and retains its temporary
directory for diagnostics. Never run this check against a normal Postal build
or a live account. Rebuild the normal app without the smoke configuration when
returning to development.

The standalone browser rail fixture and WebDriver HTTP-client regression run
on Windows. The native Linux job requires a Linux/WebKit host and is not
verified by those checks.
