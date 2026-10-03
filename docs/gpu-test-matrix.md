# Linux GPU and WebKit test matrix

Use this matrix when testing the Linux WebKitGTK window. Record Postal version,
GPU model and driver version, session type, whether the window renders, whether
media appears, and any environment overrides. Test both a normal launch and a
launch with the listed workaround removed from the environment.

| GPU driver | Wayland | X11 |
| --- | --- | --- |
| NVIDIA or nouveau | Postal detects a loaded NVIDIA module and sets `__NV_DISABLE_EXPLICIT_SYNC=1`, keeping DMA-BUF acceleration enabled. Check for a blank window or `Gdk Error 71`. | No Postal workaround is set. Check rendering and media without overrides. |
| AMD | Postal sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` to avoid the Wayland DMA-BUF failure. This can make WebKit paint on the CPU. | No Postal workaround is set. Check rendering and media without overrides. |
| Intel | Postal sets `WEBKIT_DISABLE_DMABUF_RENDERER=1` to avoid the Wayland DMA-BUF failure. This can make WebKit paint on the CPU. | No Postal workaround is set. Check rendering and media without overrides. |

The Linux startup code detects Wayland from `WAYLAND_DISPLAY` or
`XDG_SESSION_TYPE=wayland`. It checks whether an NVIDIA-family kernel module is
loaded; it does not identify which GPU renders the window on a hybrid system.
On other Wayland drivers it disables DMA-BUF. X11 receives neither setting.

Postal sets a variable only when it is absent from the process environment. A
pre-existing value takes precedence. Record the exact value when comparing
automatic behavior with a manual override.
