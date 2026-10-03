# Release memory and GPU checklist

Run this on a Linux Wayland system with a media-heavy account before publishing
release assets. Record Postal version, distro, GPU and driver, session type,
window size, and WebKit overrides. Use the same account and test chat for each
release comparison.

## Procedure

1. Start Postal and leave the chat list idle for five minutes. Record GPU memory
   and every `WebKitWebProcess` RSS value.
2. Open a chat with several pages of images or videos. Record the starting
   values, scroll down through ten message pages, then scroll back to the start.
3. Repeat the scroll loop three times. Record GPU memory and WebKit process RSS
   after each pass, plus the mounted `.vrow` count with the WebKit inspector.
4. Check the same chat with a longer archive if possible. Record total loaded
   messages and total virtual rows alongside the mounted count.
5. Reopen the same chat and verify the rail starts at the latest message. Scroll
   to older messages and verify new messages below the fold stay unread.

For NVIDIA, sample GPU memory with:

```sh
nvidia-smi --query-gpu=timestamp,name,memory.used --format=csv
```

For process RSS, run this while Postal is open:

```sh
ps -eo pid,rss,args | grep '[W]ebKitWebProcess'
```

`MessageWindow` keeps at most the configured 50–2,000 messages, with a default
of 150. The virtual list uses `bufferSize={0}`. Mounted rows should remain
limited to the viewport's visible rows as the archive grows; mounting the full
window is a failure. The README's 0.2–0.3 GB open-chat GPU reading is a reference
from one setup, not a universal limit. On the release machine, decoded media and
GPU memory should rise while media enters the viewport, then settle rather than
climbing on each repeated scroll pass. RSS may vary with WebKit caches, so
compare its trend across repeated passes and record the measurements instead of
relying on one sample.

Keep the captured values with the release review. Investigate a continued
increase after repeated passes before publishing. For the expected per-driver
Wayland settings, see the [GPU test matrix](gpu-test-matrix.md). If `nvidia-smi`
reports `Failed to initialize NVML: Driver/library version mismatch`, reboot
after the driver update and rerun the check.
