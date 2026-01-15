---
name: Windows Compatibility Report
about: Report Windows-specific issues or test results
title: '[Windows] '
labels: windows, compatibility
assignees: ''
---

## Windows Version

- **Windows version**: (e.g., Windows 11 23H2, Windows 10 22H2)
- **Architecture**: (x86_64 / ARM64)
- **Build number**: (run `winver` to check)

## mp3rgain Version

- **Version**: (run `mp3rgain version`)
- **Download source**: (GitHub Release / cargo install)

## Issue Type

- [ ] Build failure
- [ ] Runtime error
- [ ] Incorrect behavior
- [ ] Performance issue
- [ ] Compatibility report (working fine)

## Description

### If reporting an issue:

Describe the problem in detail.

### If reporting compatibility (working):

Confirm what you tested:
- [ ] `mp3rgain info` command
- [ ] `mp3rgain apply` command
- [ ] `mp3rgain undo` command
- [ ] Multiple file processing
- [ ] Files with ID3v2 tags
- [ ] VBR files
- [ ] Large files (>100MB)

## Error Messages

If applicable, paste any error messages here.

```
(paste error messages)
```

## Additional Context

Add any other context about the issue here.
