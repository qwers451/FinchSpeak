Generated test signals, not recordings. `test.*` contains two seconds of 440 Hz
(left) and 880 Hz (right), at 44.1 kHz. Tags: title `Finch test`, artist
`FinchSpeak QA`. Created with FFmpeg's `aevalsrc` filter; no third-party audio.

The VBR fixture has no Xing header and changes from one second of sine to five
seconds of seeded noise. It checks that MP3 duration comes from all packets,
rather than the demuxer's bitrate estimate.
