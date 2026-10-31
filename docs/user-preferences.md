# What the user wants: read before changing anything they will see

DropCopy's owner tests every release on their own PC (ASUS Vivobook S 16, Windows, dark mode, Hebrew
keyboard) and Galaxy S20+. These are their decisions; keep to them, and ask before departing from one.

## How to work
- **Show the result before releasing.** Build, test, send screenshots; publish a version only when they
  say "release it" (or approve the screenshots).
- Report plainly what was verified and what wasn't (Wine screenshots are not their real Windows).
- Watch cost: they work on a limited budget. Stop cleanly and leave a written plan rather than stop
  half-way through a change.

## Look and feel
- **No "AI slop."** No purple/neon gradients, glows, glassmorphism, or mono "hacker" styling. Premium
  means restrained: the app's own blue (#237EF0 → #125AC8), white, one accent, real spacing.
- **Stay loyal to the app's design** everywhere (PC windows, tray menu, web page): same blue, same phone
  drawing (Galaxy S20+-like, punch-hole, side keys), same logo (arrow into a tray).
- **The faded logo** belongs in the bottom-right corner, whole and small, never behind content and never
  cut off by an edge. Not on the web page at all.
- Text: Segoe UI Variable on Windows; titles semibold; muted secondary lines.
- Sizes must look right on a 16" laptop at 150% scaling.

## The PC side
- The terminal is not the UI: a double-click never shows or flashes a console.
- One small window-style UI: the tray icon's menu (rounded, light/dark with Windows, keyboard works).
  Header: one phone drawing (the last connected), "DropCopy", and a status light: grey Offline,
  green Connected, yellow Unstable, with the phone's name.
- The big centred window only when there is a code to scan ("Scan me" above the phone with the code).
  Reconnecting: a small phone rising above the tray (at DropCopy's tray icon), dots and
  "Looking for your phone" on its screen, then a check and "Connected to Galaxy S20+".
- The little phone in the middle of the taskbar is the Bluetooth feature only (bring the phone close →
  newest photo, a temporary file). Normal sharing shows no popup: Ctrl+V is enough.
- Updating by running a new exe must never leave two copies running.

## The phone side
- Clear, organised screens; buttons never look stitched together.
- Settings that need the user (battery, notifications, photos, updates, maker auto-start) are listed in
  one "Needs your OK" card, each with one button, shown only while missing.
- Battery matters: nothing runs in the background unless a feature is on; Bluetooth only with the screen on.

## Direction
- Setup from either side with as little as possible to do: the phone can hand the PC the app (built);
  scanning with the plain camera before the app is installed (planned, docs/roadmap.md).
- Optional "Type it for me" over Bluetooth: noted, not decided.
