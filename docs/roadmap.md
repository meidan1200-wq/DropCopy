# Next versions (ideas agreed with the user, 2026-10-31)

## v0.5: connection status and the phone's name
- **Phone name.** The phone sends its model (`Build.MODEL` → friendly name, e.g. "Galaxy S20+") with each
  check-in; the setup window says "Connected to Galaxy S20+" instead of "Phone connected".
- **No terminal flash.** Build the exe for the Windows GUI subsystem and `AttachConsole(ATTACH_PARENT_PROCESS)`
  when started from a terminal, so a double-click never shows a console.
- **Status instead of "Receiving as …"** in the tray menu header: a dot and a word, from the last time the
  phone was heard (hello every 15 s, check-ins, transfers):
  grey = offline, green = connected, yellow = unstable (hellos missed / slow or failed connects).
- **Phones instead of the app logo** in that header: a small drawing of each paired phone (S20+, then
  S26 Ultra next to it, …), each showing the blue screen with the DropCopy logo faded on it.

## v0.5: reconnecting looks different from first pairing
- First pairing: "Scan me" with the code, as now.
- Reconnecting (already paired): instead of the dots, the small phone with its screen blue and the DropCopy
  logo filling most of it, faded; it turns into the connected state when the phone answers.

## Later: setup from either side, with no app hunting
- **Scan with the plain camera first.** The QR becomes an https link (e.g. a GitHub Pages page) with the
  pairing data after `#` (never sent to any server). Without the app: the page offers the APK (or, once on
  Google Play, opens the store). The page also copies the pairing link to the clipboard; the app reads it
  on its first launch and pairs by itself. With the app installed, Android App Links open the app directly.
  On Google Play the Install Referrer API can carry the pairing through the install instead.
- **Phone first, then the PC.** The app shows "Set up your PC: open http://<phone-ip>:8080 on it". The phone
  serves DropCopy-Setup.exe over the local network with a one-time pairing blob appended to the file (an exe
  still runs with data after it); the PC setup reads it from its own file, finds the phone on the network
  and completes pairing with no scan at all. Not sci-fi: plain LAN HTTP plus the existing protocol.
