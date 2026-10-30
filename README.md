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

1. Run `dropcopy.exe`. It prints a QR code.
2. When Windows Firewall asks, tick **Private networks** → Allow. Your Wi-Fi must be set to *Private*
   (Settings → Network → Wi-Fi → your network → Private), or inbound connections are blocked.
3. Optional: `dropcopy setup` — starts hidden at sign-in. Run it from an administrator terminal to
   also add the firewall rule for you.

**Phone**

1. Install the APK (allow "install unknown apps" for your browser/files app).
2. Open DropCopy → **Scan pairing code** → point at the QR. It checks the connection right away and
   the PC prints `checked in`.
3. Optional: pull down Quick Settings → edit → add the **Send clipboard** tile.

## Sending

| You do on the phone | What happens |
|---|---|
| Share → **DropCopy** (or the PC's icon in the share sheet's top row) | Sent immediately; the sheet just closes. A toast confirms. |
| Copy something, tap the **Send clipboard** tile | The phone's clipboard (text or image) goes to the PC's clipboard. |
| Turn on **Auto-send new screenshots** (or camera photos), then take a screenshot | Zero taps: it's on the PC's clipboard a moment later. |

On the PC every file is saved to `Pictures\DropCopy` (change with `dropcopy dir D:\Somewhere`), and:

- **one image** → clipboard holds it as a *picture and a file*: Ctrl+V in WhatsApp/Discord/Slack/Gmail/
  Word/Paint pastes the image; Ctrl+V in Explorer pastes the file.
- **several files** → clipboard holds the file list (paste in Explorer, or attach in a chat/mail).
- **text / links** → clipboard holds the text.

`--clip image|file|off` changes that. Other commands: `dropcopy --help`.

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
Over Wi-Fi expect the link speed (typically 20–80 MB/s) plus a few ms of setup.

## Working around the OS limits

| Limit | Workaround |
|---|---|
| Android won't let apps read the clipboard in the background | The QS tile opens an invisible window for an instant (focused apps *may* read it), sends, closes. Auto-send watches for new screenshots instead of the clipboard. |
| Wi-Fi without internet makes Android route traffic over mobile data | The app pins itself to the Wi-Fi network before sending. |
| The PC's IP changes (DHCP) | Authenticated UDP discovery runs in parallel with the direct connect; the new address is remembered. |
| Windows clipboard formats differ per app | Picture as CF_DIB + original PNG/JFIF + CF_HDROP file list in one clipboard write. |

What it does **not** do: AirDrop's peer-to-peer Wi-Fi (AWDL). Phone and PC need to be on the same network,
or the PC connected to the phone's hotspot. Guest networks with "client isolation" block it.

## Security

Pairing exchanges a random 256-bit key through the QR code (it never crosses the network). Every transfer is
authenticated and encrypted with keys derived from it; discovery replies are authenticated too, so nothing on
the network can impersonate your PC or push files to it. `dropcopy pair --new` revokes every phone.
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

The release APK is signed with your local debug key (fine for sideloading); a build from another machine
has a different key, so uninstall the old one first.

## Tests

```bash
cd pc && cargo test --release                      # protocol, crypto, files, discovery (12 tests)
cd android && ./gradlew testReleaseUnitTest        # Android glue on Robolectric (6 tests)
# Android tests against a live receiver, and the phone's protocol code on a desktop JVM:
DROPCOPY_TEST_LINK="$(dropcopy pair | grep -o 'dropcopy://[^ ]*')" DROPCOPY_TEST_DIR="$(dropcopy dir)" ./gradlew testReleaseUnitTest
javac -d /tmp/jt app/src/main/java/dev/dropcopy/Proto.java jvmtest/ProtoTest.java && java -cp /tmp/jt ProtoTest "<link>" photo.jpg
```

Wire format: [PROTOCOL.md](PROTOCOL.md).
