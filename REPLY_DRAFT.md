# Reddit Reply Draft for mjb2012

## Reply

Thank you for the continued testing and detailed report! You've identified another critical bug in the loudness calculation.

### About the project and AI usage

Yes, this project does use AI assistance for development. However, I do perform manual testing, create GitHub issues for tracking, and verify fixes for each problem reported. Your reports have been invaluable for finding these issues.

### Some context on why ReplayGain has these bugs

mp3rgain originally started as a spin-off from my other project, [headroom](https://github.com/M-Igashi/headroom), which depended on the original mp3gain for global gain adjustments. My goal was to eliminate that dependency while also reimplementing mp3gain, which hadn't been maintained for years.

Because of this origin, **the core functionality (reading/writing MP3 global gain) was the priority, and ReplayGain analysis was implemented later as a secondary feature**. This led to insufficient testing of the loudness calculation algorithm. I should have been more careful here.

### What I found

After investigating your report, I discovered the loudness calculation was fundamentally broken:

1. **Missing calibration constant**: The original mp3gain uses `PINK_REF = 64.82` as a calibration reference. My implementation incorrectly used `89.0` directly in the gain formula.

2. **Incorrect algorithm structure**: The original algorithm accumulates squared samples across 50ms windows and builds a histogram for 95th percentile calculation. My implementation was processing samples incorrectly.

I've rewritten the analysis to match the original `gain_analysis.c` more closely. See [issue #39](https://github.com/M-Igashi/mp3rgain/issues/39) for details.

### Next steps

I'll revert the global gain on my personal MP3 music library and run ReplayGain tests against the original mp3gain to verify the fix produces matching results. Once I've confirmed it works correctly, I'll release the update.

A proper test suite with reference files is definitely needed - your feedback reinforces how critical that is.

Thank you for your patience and thorough testing!
