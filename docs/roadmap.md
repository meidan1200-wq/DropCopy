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
- **Phone first, then the PC.** *(Built after v0.5.0: `PcSetup.java`, `pc/src/handoff.rs`.)* The app shows "Set up your PC: open http://<phone-ip>:8080 on it". The phone
  serves DropCopy-Setup.exe over the local network with a one-time pairing blob appended to the file (an exe
  still runs with data after it); the PC setup reads it from its own file, finds the phone on the network
  and completes pairing with no scan at all. Not sci-fi: plain LAN HTTP plus the existing protocol.

## Later, maybe: "Type it for me" over Bluetooth (noted 2026-10-31, not decided)
A PC without DropCopy can't be told to open a page over Bluetooth. The one route: the phone presents
itself as a Bluetooth keyboard (Android 9+ `BluetoothHidDevice`), and after the user pairs it once in
Windows' Bluetooth settings it types Win+R, the setup address, Enter. Caveats: the one-time pairing is
about as much effort as typing the address; a non-English keyboard layout (Hebrew) turns the typed
address into the wrong letters; not every phone maker enables the profile; a phone that types on a PC
looks suspicious to people and antivirus. If built: an optional "Type it for me" button, shown only
where supported, with the typed address staying the main path.

## Self-checks for every release (from real use)
- **Phone name after an upgrade.** If a v0.5+ phone first reaches a PC still on v0.4.x, it marks that PC
  "older" and leaves out its name and options for 24 h, so the tray shows the app logo and a plain
  "Connected" until then (seen 2026-10-31; fine once the phone updated). Next version: let check-ins
  always try the full records and clear the mark on success. Before releasing, check: phone updated
  first, PC second → name appears in the tray on the next check-in.
- **Release builds:** a test that needs a fixed port must skip, not fail, when the port is taken
  (`Server::discovering()`); stress tests must delete their files (a full disk failed a run).
