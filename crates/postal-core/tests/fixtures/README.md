`tiny-video.mp4` and `portrait-video.mp4` are generated red H.264 baseline clips;
no user media is included.

```sh
ffmpeg -f lavfi -i color=c=red:s=320x240:r=10:d=1 -c:v libx264 -profile:v baseline -level:v 3 -pix_fmt yuv420p -frames:v 10 -movflags +faststart tiny-video.mp4
ffmpeg -f lavfi -i color=c=red:s=240x320:r=10:d=1 -c:v libx264 -profile:v baseline -level:v 3 -pix_fmt yuv420p -frames:v 10 -movflags +faststart portrait-video.mp4
```

Both dimensions stay above the Windows H.264 decoder's 48×48 minimum:
https://learn.microsoft.com/en-us/windows/win32/medfound/h-264-video-decoder

`tone.m4a` is one second of a 440 Hz sine encoded as AAC-LC, so the audio
decoder has a real MP4/AAC file to chew on:

```sh
ffmpeg -f lavfi -i "sine=frequency=440:duration=1" -c:a aac -b:a 32k tone.m4a
```
