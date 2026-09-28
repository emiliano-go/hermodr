`tiny-video.mp4` is a generated red H.264 baseline clip; no user media is included.

```sh
ffmpeg -f lavfi -i color=c=red:s=320x240:r=10:d=1 -c:v libx264 -profile:v baseline -level:v 3 -pix_fmt yuv420p -frames:v 10 -movflags +faststart tiny-video.mp4
```

320×240 stays above the Windows H.264 decoder's 48×48 minimum:
https://learn.microsoft.com/en-us/windows/win32/medfound/h-264-video-decoder
