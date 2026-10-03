# Troubleshooting

## Blank or black window

Postal renders its Linux interface with WebKitGTK. On Wayland, DMA-BUF can fail
with `Gdk Error 71` and leave a blank or black window.

Postal applies these workarounds when it starts:

- With an NVIDIA or nouveau kernel module loaded, it sets
  `__NV_DISABLE_EXPLICIT_SYNC=1` and keeps WebKit's DMA-BUF renderer enabled.
- With other Wayland drivers, it sets `WEBKIT_DISABLE_DMABUF_RENDERER=1`.
  This avoids the DMA-BUF failure but can make WebKit paint on the CPU.
- On X11, Postal sets neither variable.

An environment value that already exists takes precedence, even if it is `0`.
To check Postal's defaults from a terminal, close the app and launch it with
both overrides removed:

```sh
env -u __NV_DISABLE_EXPLICIT_SYNC -u WEBKIT_DISABLE_DMABUF_RENDERER postal
```

Replace `postal` with the command or AppImage path used to start your install.
To test the fallback on Wayland, launch with
`WEBKIT_DISABLE_DMABUF_RENDERER=1`. To test the NVIDIA explicit-sync workaround,
launch with `__NV_DISABLE_EXPLICIT_SYNC=1`. See the
[GPU test matrix](gpu-test-matrix.md) for the expected behavior by driver and
session.

## Find the log and raise verbosity

Open **Settings → About → Open log** to open the active `postal.log` in the
application data directory. Its default path is:

- Linux: `$XDG_DATA_HOME/com.postal.app/postal.log`, or
  `~/.local/share/com.postal.app/postal.log` when `XDG_DATA_HOME` is unset.
- macOS: `~/Library/Application Support/com.postal.app/postal.log`.
- Windows: `%APPDATA%\com.postal.app\postal.log`.

The app-data directory can be overridden by Tauri configuration, and the UI
button reveals the exact path in use. The previous file is `postal.log.old`.
Postal rotates the log after it grows past 5 MiB. These paths follow Tauri's
[`app_data_dir()` defaults](https://docs.rs/tauri/latest/tauri/path/struct.PathResolver.html#method.app_data_dir).

The default release log includes Postal and UI messages at `info`, plus warnings
and errors. Set `RUST_LOG` to override the filter. For a verbose Linux run:

```sh
RUST_LOG=debug postal
```

Debug logs can contain private conversation or account details. Review and
redact the excerpt before attaching it to a report.

## NVIDIA reports a driver/library mismatch

If `nvidia-smi` reports `Failed to initialize NVML: Driver/library version mismatch`,
reboot after the driver package update. The loaded kernel module and the user
space NVML library must come from the same driver version. Retry `nvidia-smi`
after reboot, then start Postal again. [NVIDIA's upgrade guide](https://docs.nvidia.com/base-os/dgx-os-5/upgrading_dgx_os.html)
documents this error when the system has not restarted after a driver update.

## Inspect and clean Linux core dumps

`systemd-coredump` may store large WebKit process dumps under
`/var/lib/systemd/coredump`. List available core dumps with:

```sh
coredumpctl list
```

Use the `EXE` column to find `WebKitWebProcess` entries.

Use the PID from the list to inspect metadata or open the dump in GDB:

```sh
coredumpctl info PID
coredumpctl debug PID
```

`debug` starts GDB. At the GDB prompt, `thread apply all bt full` prints all
thread backtraces. Keep the exact Postal build from the crash for symbol lookup;
release builds strip debug information, so an unstripped matching build gives
better Postal frames. WebKit system library symbols depend on the distro's
debug-symbol packages.

`coredumpctl` has no `vacuum` command. systemd-tmpfiles applies the configured
retention rules to core files. To run the system cleanup now:

```sh
sudo systemd-tmpfiles --clean
```

This applies all configured tmpfiles cleanup rules on the machine, not only
Postal dumps. systemd also removes core files automatically according to its
retention configuration. See the
[systemd-coredump manual](https://www.freedesktop.org/software/systemd/man/latest/systemd-coredump.html)
and [coredumpctl manual](https://www.freedesktop.org/software/systemd/man/latest/coredumpctl.html).

Postal does not set an `RLIMIT_CORE` for itself or its WebKit children. Native
core data can preserve WebKit state that Postal's own log cannot capture. Postal
keeps the operating system's capture and retention policy in control instead of
applying an unmeasured size cap that may remove useful crash evidence. The app
log separately records Postal panics with a Rust backtrace.

## Reinstall on Linux

Run the install command from the README to rebuild and reinstall the current
`master` version:

```sh
curl -fsSL https://raw.githubusercontent.com/emiliano-go/postal/master/scripts/install.sh | sh
```

The installer also accepts `POSTAL_REF=v0.1.0` to build a tag or commit, and
`--debug` for a faster unoptimized build. Reinstalling does not replace the
message archive. To build from an existing clone, use `scripts/install-dev.sh`.
