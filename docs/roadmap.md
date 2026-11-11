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

## v0.6 (built 2026-11-11, being tested): one share icon, PC to phone, PC to PC
See README ("From the PC") and PROTOCOL.md ("v2"). Choices made, to confirm with the user:
- *Receive from your PCs* is **on by default** (a sleeping UDP socket while the screen is on, no
  Bluetooth; it does keep the low-priority DropCopy notification). Turn off in the app.
- The single share icon is labelled with the PC's name (one PC) or "My PC" (several).

## Next: phone to phone, with the warp animation (asked 2026-11-11)
1. **Pairing phones.** Phone B shows a QR code in the app (its own id and key, like a PC); phone A scans
   it. Same v1 handshake; phone B runs a small TCP receiver only while its screen is on (or a
   foreground "Receiving" screen is open), found by the existing UDP discovery.
2. **Share target.** The top-row icon list gains the paired phone (e.g. "Galaxy S26"), still one icon
   per destination kind; "My PC" keeps picking among PCs.
3. **Warp animation (picture shares between phones only).** Sender: ShareActivity stays visible
   ~600 ms: the picture shrinks into a rounded card and is pulled into a ripple that bends toward the
   top of the screen (RenderEffect blur + a scale/translate path on Android 12+; scale and fade on
   10–11). Receiver: a full-screen-intent notification (Android allows it for a short, user-visible
   event) opens a translucent activity where the picture lands out of the same ripple, then settles
   into the notification. App colours (#237EF0), no neon or glow.
4. **Nearby fallback, maybe.** Without shared Wi-Fi, Google's Nearby Connections (Play services) gives
   Bluetooth/Wi-Fi Direct transport. Adds a dependency; only if same-network use isn't enough.
