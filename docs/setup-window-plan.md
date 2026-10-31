# Plan: the setup window (replaces the terminal on double-click)

Decided with the user: one medium window (~480×560), centred on screen, no terminal.

## Look
- Background: DropCopy blue (#1A73E8 → slightly darker gradient), the app logo large and faint (~8% white) behind.
- White text, Segoe UI Variable (GDI text, like `src/menu.rs`); shapes drawn in Rust (SDF, like `phone_art.rs`/`menu.rs`).

## States
1. **Not paired** (`!cfg.is_paired()`): the S20+ phone (`phone_art::render_code`) centred, QR on its screen,
   "Scan me" above in white, small hint below ("Open DropCopy on your phone").
2. **Paired, waiting**: three white dots in the middle, pulsing in sequence (scale+opacity, ~1.2 s cycle).
3. **Connected**: dots morph into the phone with a check mark + "Connected to <phone>" (fade/scale-in,
   ~400 ms), then the window fades out after ~3 s.

## How it hooks in
- `main.rs::launch()`: keep `setup::ensure_background()`; replace the console banner + `tail_log` with
  `setup_window::run(cfg)` and hide the console (`FreeConsole` / build as windows subsystem with
  `AttachConsole(ATTACH_PARENT_PROCESS)` for CLI commands).
- "Connected" signal: watch `log.txt` for a line starting `  <-` (as `tail_log` does today), or the `paired` file appearing.
- Layered window like `popup.rs` (UpdateLayeredWindow, per-pixel alpha), 60 fps timer only while animating.

## Phone name (needs a new APK)
- Phone sends `Build.MODEL` (e.g. "SM-G986B") / a friendly name in a new T_OPTS-like record or the
  check-in; PC logs `<- Galaxy S20+ checked in`. Until then the text is "Phone connected".

## Tests
- Pure-Rust render tests per state (like `menu.rs` tests), Wine screenshot of each state.
