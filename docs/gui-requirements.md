# mp3rgui System Requirements

`mp3rgui` is built on [egui](https://github.com/emilk/egui) via `eframe`, which
renders through **OpenGL 2.0 or newer**. If no OpenGL 2.0+ context is available,
the GUI cannot start and exits with an explanatory dialog.

The CLI (`mp3rgain`) has no graphics dependency and is unaffected by everything
on this page.

## Is this a real constraint?

Usually not. OpenGL 2.0 dates from 2004, so no GPU made in the last two decades
lacks the capability. **The limit is never the hardware — it is whether a driver
exposes OpenGL to the process.** That differs sharply by platform:

| Platform | Situation |
|----------|-----------|
| macOS | Every supported Mac provides OpenGL 4.1. Not affected. |
| Linux | Even with no GPU, Mesa's `llvmpipe` software rasteriser provides far above OpenGL 2.0. Effectively not affected. |
| Windows | The inbox Microsoft Basic Display driver provides only OpenGL 1.1. OpenGL 2.0+ requires a vendor GPU driver. **This is where the limitation bites.** |

## Where it actually fails

Ordered by how likely you are to hit it.

### 1. Remote Desktop (RDP) — the one that affects ordinary users

Windows' standard RDP display driver typically exposes only OpenGL 1.1,
regardless of what GPU the physical machine has. A desktop where `mp3rgui`
runs perfectly will fail to start the moment you reach it over Remote Desktop.

Workarounds:

- **Use the `mp3rgain` CLI over RDP.** It covers the same functionality.
- Use a remote desktop tool that forwards the real GPU session instead
  (VNC attached to the console session, Parsec, Steam Link, and similar).
- On hypervisors that support GPU partitioning (GPU-P), assigning a virtual
  GPU to the guest restores OpenGL. Note that RemoteFX vGPU, the older
  mechanism, was removed from Windows in 2020 for security reasons.

### 2. Virtual machines without 3D acceleration

Hyper-V's synthetic video adapter, or VirtualBox / VMware with 3D
acceleration switched off. Enable 3D acceleration for the guest, or use the
CLI.

### 3. Windows Sandbox

The disposable sandbox environment does not provide a GL 2.0+ driver in
typical configurations.

### 4. Freshly installed Windows

Between OS installation and the first vendor GPU driver arriving via Windows
Update, only the basic display driver is present. Temporary — installing the
graphics driver resolves it.

### 5. Server Core and headless installs

No window system at all. The GUI is not the right tool here; use the CLI.

## Why there is no software-rendering fallback

`eframe` can also render through `wgpu` (DirectX 12 / Vulkan / Metal), which
would likely start in some of the cases above. It is deliberately not enabled:

- It adds a large amount of code to the binary. `mp3rgui.exe` is already an
  unsigned, statically linked Rust binary that periodically trips Microsoft
  Defender's ML heuristics (see the note in the [README](../README.md)), and
  binary size is a contributing factor.
- Every affected scenario already has a working answer — the CLI — that needs
  no graphics stack at all.

This was evaluated and declined in
[issue #282](https://github.com/M-Igashi/mp3rgain/issues/282). If you have a
use case where the CLI is genuinely not a substitute, please say so on that
issue; the decision is worth revisiting with a concrete case behind it.

## What the failure looks like

Startup failures are reported with the likely cause and what to try, for
example:

```
mp3rgain GUI could not start.

This computer has no usable OpenGL 2.0+ driver, which the GUI requires.
Common causes are a missing or outdated graphics driver, a virtual
machine without 3D acceleration, or a remote desktop session that does
not forward OpenGL.

What you can try:
  - Install or update your graphics driver.
  - On a virtual machine, enable 3D acceleration for the guest.
  - Use the mp3rgain command-line tool instead. It does everything the GUI
    does and needs no graphics driver:
    https://github.com/M-Igashi/mp3rgain

Details: egui_glow: OpenGL: egui_glow requires opengl 2.0+.
```

The raw error is kept under `Details:` — please include it when reporting a
startup problem.
