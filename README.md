# DropCopy

Share on your Android phone, press **Ctrl+V** on your PC. Local network only — no cloud, no account,
nothing leaves your Wi-Fi.

```
phone: Share → DropCopy        PC: file saved to Pictures\DropCopy
       (or the PC's own icon)       + put on the clipboard → Ctrl+V anywhere
```

- **`pc/`** — the receiver: one Rust binary (~1.5 MB `.exe`), runs in a terminal, no runtime, no installer.
- **`android/`** — the phone app: plain Java, no UI framework (~640 KB APK).

## Setup (once, ~1 minute)

**PC (Windows)**

1. Download `DropCopy-Setup.exe` from the latest release and double-click it. It sets itself up to run in the background and to start with Windows
   (installed to `%LOCALAPPDATA%\DropCopy`), then shows the pairing QR code and live activity. **Closing the
   window does not stop it.** With no network it waits quietly and picks up when the Wi-Fi is back.
   Running a newer download the same way upgrades it; an older one never replaces a newer install.
2. When Windows Firewall asks, tick **Private networks** → Allow. (From an administrator terminal,
   `dropcopy setup` adds the rule itself, so Windows never asks.)
3. Log: `%APPDATA%\DropCopy\log.txt`. Remove it all with `dropcopy unsetup`. `dropcopy serve` receives
   in the window only, the old way.

**Phone**

1. Install the APK (allow "install unknown apps" for your browser/files app).
2. Open DropCopy. If the phone isn't paired, it asks the network, the PC shows its pairing code above the
   taskbar (as a small phone), and the scanner opens: point it at the code. That's the whole pairing,
   and it works the same after reinstalling the app. The PC remembers its key, so phones paired before
   keep working.
3. If anything still needs your OK on the phone (battery saving, notifications, installing updates), a
   **Needs your OK** card lists exactly that, with one button each.
4. **Show on PC when near** opens a small setup window: Bluetooth on, Nearby devices allowed, and a live
   meter of how strongly your laptop hears the phone. No Bluetooth pairing is needed. Tap the row again
   any time to reopen it.
5. Optional: pull down Quick Settings → edit → add the **Send clipboard** tile.

## Sending

| You do on the phone | What happens |
|---|---|
| Share → **DropCopy** (or the PC's icon in the share sheet's top row) | Sent immediately; the sheet just closes. A toast confirms. |
| Copy something, tap the **Send clipboard** tile | The phone's clipboard (text or image) goes to the PC's clipboard. |
| Turn on **Auto-send new screenshots** (or camera photos), then take a screenshot | Zero taps: it's on the PC's clipboard a moment later. |

**Bring the phone close (the little phone).** Turn on *Show on PC when near* in the app. Hold the phone next
to your laptop and it sends its newest photo; a small phone with that photo on its screen rises from the
middle of the taskbar. Drag it into any app (chat, mail, Word, a folder) to drop it there, click to open it,
or leave it and it slides away; hovering keeps it up. That preview is a temporary file
(`%APPDATA%\DropCopy\nearby`), never in Pictures\DropCopy: it is deleted when the little phone goes, or an
hour later if you dragged or opened it, and anything left over is removed at the next start. Normal sharing
shows no popup: it is already on the clipboard for Ctrl+V.

It works over Bluetooth Low Energy: while the screen is on, the phone broadcasts a token that changes every
minute and only your PC can recognise; the PC watches its signal strength and then asks the phone (over
Wi-Fi, authenticated) for the photo. Laptops work best (the antenna is in the lid or keyboard). The setup
window in the app shows the live signal; `dropcopy set near_rssi -50` makes "close" closer (higher) or
farther (lower). `dropcopy set popup off` turns the little phone off.

**The tray icon.** DropCopy's icon (the same as the phone app's) sits in the notification area while it runs.
Click it to open the received pictures; right-click for DropCopy's own menu (rounded, light or dark to match
Windows, keyboard too): show the pairing code, **check for updates** (it
installs the new version and restarts, with a Windows notification saying what happened), or quit. That menu
is the only window-style part of the PC side; everything else is the terminal.

**Updating by running a new exe** never leaves two copies: setup first asks every other DropCopy process
to quit (any version, any folder, any download name such as `dropcopy (1).exe`), ends whatever is left,
then swaps the installed copy and starts one. Two setups at once take turns. A download older than what is
installed never replaces it.

**Delete from PC after pasting** (phone app, asks twice). Pictures you send are deleted from
Pictures\DropCopy 30 seconds after the first paste, or 5 seconds after you copy anything else on the PC, or
at the next start if never pasted. Pasting the image again keeps working (it is on the clipboard); pasting it
*as a file* works only within those 30 seconds. If Windows clipboard history (Win+V) is on, it may read the
picture at once, which counts as a paste.

**Updates.** Both apps update themselves from this repository's releases. The PC's background copy checks
shortly after start and every 6 hours, verifies the download's SHA-256, swaps itself and restarts; run
`dropcopy update` to do it now. The phone checks daily on Wi-Fi and installs through Android's installer;
from the second update on, Android 12+ installs it without asking. Settings: `dropcopy set` (PC), the *App*
section (phone).

On the PC every file is saved to `Pictures\DropCopy` (change with `dropcopy dir D:\Somewhere`), and:

- **one image** → clipboard holds it as a *picture and a file*: Ctrl+V in WhatsApp/Discord/Slack/Gmail/
  Word/Paint pastes the image; Ctrl+V in Explorer pastes the file.
- **several files** → clipboard holds the file list (paste in Explorer, or attach in a chat/mail).
- **text / links** → clipboard holds the text.

`--clip image|file|off` changes that. `dropcopy doctor` checks network, drag and drop and updates.
Other commands: `dropcopy --help`.

## Why it is fast

- **No discovery wait.** The phone connects straight to the PC's last address *and* broadcasts a
  discovery packet at the same moment; whichever answers first wins. Same network: a few ms. PC got a new
  address: discovery finds it, typically within ~5–20 ms on a LAN.
- **One round trip** of handshake after TCP connect, then the file streams in 512 KB AES-256-GCM records
  (hardware AES on both ends — never the bottleneck; Wi-Fi is).
- **Clipboard in ~1 ms.** On Windows the file list and the original PNG/JPEG bytes go on the clipboard the
  instant the file lands; the bitmap is decoded in the background and handed over only when an app asks for
  it (*delayed rendering*). A 12 MP photo doesn't make you wait for a decode.
- **No UI in the path.** The share target is an invisible activity that hands off to a background service
  and closes; you go straight back to what you were doing.
- Photos keep their orientation (EXIF) when pasted as a picture.

Measured in development (loopback, not Wi-Fi): 9 MB photo received in 34 ms; clipboard ready in 3–25 ms.
Also run end to end on Android 11 (emulator, software-only) against the Windows build: pairing, Share → DropCopy,
clipboard send and screenshot auto-send all delivered byte-identical files to the PC clipboard.
Over Wi-Fi expect the link speed (typically 20–80 MB/s) plus a few ms of setup.

## Working around the OS limits

| Limit | Workaround |
|---|---|
| Android won't let apps read the clipboard in the background | The QS tile opens an invisible window for an instant (focused apps *may* read it), sends, closes. Auto-send watches for new screenshots instead of the clipboard. |
| Wi-Fi without internet makes Android route traffic over mobile data | The app races the connection over every network the phone has (Wi-Fi explicitly, and the default route) — whichever reaches the PC wins. |
| The phone is off Wi-Fi, or the PC is off | The send is queued on the phone and goes out by itself when a network returns (also after a reboot); no error to dismiss. |
| The PC's IP changes (DHCP) | Authenticated UDP discovery runs in parallel with the direct connect; the new address is remembered. |
| Windows clipboard formats differ per app | Picture as CF_DIB + original PNG/JFIF + CF_HDROP file list in one clipboard write. |

What it does **not** do: AirDrop's peer-to-peer Wi-Fi (AWDL). Phone and PC need to be on the same network,
or the PC connected to the phone's hotspot. Guest networks with "client isolation" block it.

## Security

Pairing exchanges a random 256-bit key through the QR code (it never crosses the network). Every transfer is
authenticated and encrypted with keys derived from it; discovery replies are authenticated too, so nothing on
the network can impersonate your PC or push files to it. `dropcopy pair --new` revokes every phone.
The "show your pairing code" request carries nothing secret; the PC answers it at most every 20 s with its
name, and the code (which holds the key) only ever appears on the PC's own screen. Turn the popup off with
`dropcopy set popup off`.
Settings: `%APPDATA%\DropCopy\config.txt`.

## Building

```bash
# PC (native)
cd pc && cargo build --release            # target/release/dropcopy(.exe)
# Windows .exe from Linux
rustup target add x86_64-pc-windows-gnu   # plus mingw-w64
cd pc && cargo build --release --target x86_64-pc-windows-gnu

# Android (JDK 17+, Android SDK with platform 35)
cd android && ./gradlew assembleRelease   # app/build/outputs/apk/release/app-release.apk
```

Releases are built by GitHub Actions (`.github/workflows/release.yml`; Actions tab → Release → Run workflow,
enter a version). The APK is signed with the release key from the `DROPCOPY_KEYSTORE_B64` and
`DROPCOPY_KEYSTORE_PASSWORD` secrets, so every version installs over the last; the workflow refuses to
publish without them. Local builds without the secrets use the debug key and can't update a released app.

## Tests

```bash
cd pc && cargo test --release          # 38 tests: protocol, files, discovery, proximity, cleanup, updates, and…
cargo test --release --test stress -- --nocapture   # …the limits (numbers below)
cd android && ./gradlew testDebugUnitTest            # 80 tests on Robolectric, incl. every Android version 10–15
DROPCOPY_SHOTS=/tmp/shots ./gradlew testDebugUnitTest --tests '*Screenshot*'   # renders the screens
cargo run --release --example phone_preview photo.jpg /tmp   # renders the taskbar phone
cargo run --example make_icon assets                          # redraws the app icon
# Android tests against a live receiver:
DROPCOPY_TEST_LINK="$(dropcopy pair | grep -o 'dropcopy://[^ ]*')" DROPCOPY_TEST_DIR="$(dropcopy dir)" DROPCOPY_TEST_HOME="%APPDATA%/DropCopy" ./gradlew testDebugUnitTest
```

**Limits the receiver is held to** (`pc/tests/stress.rs`, measured on a 4-core cloud VM over loopback; a real
phone is limited by its Wi-Fi, typically 20–80 MB/s):

| | |
|---|---|
| One 512 MB video | 179 MB/s, memory +2 MB (streams in 512 KB records) |
| One share of 500 photos | 84 ms, all intact |
| 24 phones sending 3 MB at once | 0.25 s |
| A check-in (what every send starts with) | 0.2 ms |
| Idle | 0 ms CPU in 3 s |
| 200 hostile connections open (garbage, half hellos, a claimed 4 GB record) | a real phone still gets through in ms, memory flat |
| 400 silent sockets | past 256 closed at once, the rest after 4 s; then phones get in again |
| 20 000 junk discovery packets | still answers |
| Names like `../../x`, `C:\Windows\win.ini`, `CON`, 800 characters | all saved inside the folder, made safe |
| A transfer cut off midway | nothing half-written left behind |
| A tampered or replayed record | refused |

**On the phone** (`CompatTest`, `BatteryTest`): the main flows run on Android 10, 11, 12, 13, 14 and 15, each
asking only for the permissions that version has. Nothing runs in the background until a feature is turned on;
Bluetooth and the PC link run only while the screen is on, and the link sleeps between its 15-second hellos
instead of polling; no wake locks; the update check waits for Wi-Fi, retries wait for any network. On Xiaomi,
Huawei, Honor, Oppo, Realme, OnePlus, Vivo and Asus, whose own battery managers stop background apps, the
*Needs your OK* card opens that maker's auto-start screen.

Wire format: [PROTOCOL.md](PROTOCOL.md).
