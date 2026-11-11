# DropCopy protocol v1 (and v2 additions)

Implementations: `pc/src/proto.rs` (receiver and sender), `android/app/src/main/java/dev/dropcopy/Proto.java`
(sender). All integers big-endian.

## Pairing

The receiver owns a random 8-byte **id** and a random 32-byte **key K**, shown once as a QR code:

```
dropcopy://pair?v=1&id=<16 hex>&k=<base64url K, no padding>&n=<percent-encoded name>&p=<tcp port>&a=<ipv4>
```

`a` is only a first guess at the address; discovery corrects it.

## Discovery — UDP port 48651

```
phone → broadcast   "DCD1" | id[8] | nonce[16]                               (28 bytes)
PC    → phone       "DCR1" | id[8] | port u16 | HMAC-SHA256(K, "dropcopy/v1/disc" | nonce | port)[0..16]   (30 bytes)
```

The PC answers only requests carrying its own id. The phone accepts only replies whose tag verifies, so a
reply cannot be forged without K. The phone sends to every interface's broadcast address and 255.255.255.255,
at 100, 200, 400, 800 ms…, while *simultaneously* opening TCP to the last known address. First connection wins;
the others are closed before sending anything (the PC ignores them silently).

### Asking for the pairing code (unpaired phone)

```
phone → broadcast   "DCQ1" | nonce[8]                                         (12 bytes)
PC    → phone       "DCA1" | nonce[8] | name (utf-8, ≤ 64 bytes)
```

A phone with no pairing (or whose PC no longer answers its id) asks; every PC that hears it answers with its
name and shows the pairing QR code on its own screen, at most once per 20 s. Nothing secret crosses the network:
the key only ever travels in the QR code. The PC takes the code down when a phone connects with the key.

### "Does my PC see me?" (proximity setup)

```
phone → PC/broadcast "DCN1" | id[8] | nonce[16]                               (28 bytes)
PC    → phone        "DCM1" | id[8] | body[7] | HMAC-SHA256(K, "dropcopy/v1/nearq" | nonce | body)[0..16]
                     body = state u8 (0 off, 1 no Bluetooth, 2 scanning) | dBm i16 (−32768 = not heard
                            in 5 s) | threshold i16 | age ms u16
```

Polled about once a second while the phone's proximity setup window is open, to show a live signal meter.

### Proximity: "here I am" and "send your newest photo"

```
phone → PC/broadcast "DCH1" | id[8] | nonce[16] | HMAC-SHA256(K, "dropcopy/v1/hello" | nonce)[0..16]   (44 bytes)
PC    → phone        "DCU1" | id[8] | nonce[16] | HMAC-SHA256(K, "dropcopy/v1/pull"  | nonce)[0..16]   (44 bytes)
```

While proximity is on and its screen is on, the phone says hello every 15 s from the socket it listens on.
When Bluetooth shows it came close, the PC sends a pull to that address (twice), and the phone answers with
an ordinary transfer: OPTS 0x01, then its newest image.

### Stopping the running copy (same machine only)

```
local → 127.0.0.1   "DCS1"                                                    (4 bytes)
```

Sent by a newer copy that is installing itself; the PC accepts it only from a loopback address and exits.

## Transfer — TCP port 48650

```
C → S   "DCP1" | cn[16]
S → C   "DCP1" | sn[16] | HMAC(K, "dropcopy/v1/srv" | cn | sn)              client verifies: PC holds K
        k_c2s = HMAC(K, "dropcopy/v1/c2s" | cn | sn)
        k_s2c = HMAC(K, "dropcopy/v1/s2c" | cn | sn)
C → S   records under k_c2s …
S → C   one record under k_s2c
```

A **record** is `len u32 | AES-256-GCM(plaintext)` with a 16-byte tag, nonce `00000000 | counter u64`
(counter starts at 0 per direction), empty AAD, plaintext ≤ 1 MiB. The client is authenticated by its first
record decrypting.

Plaintext = `type u8 | body`:

| type | name | body |
|---|---|---|
| 0x01 | FILE | size u64 (all ones = unknown) \| name_len u16 \| name \| mime_len u16 \| mime |
| 0x02 | DATA | bytes of the current file |
| 0x03 | FILE_END | — |
| 0x04 | TEXT | UTF-8, for the clipboard |
| 0x05 | END | — sender is done |
| 0x06 | OPTS | flags u8, before any file: 0x01 nearby preview (temporary, shown on the taskbar phone, not on the clipboard), 0x02 delete from the PC after pasting |
| 0x10 | OK | count u16 (PC → phone) |
| 0x11 | ERR | UTF-8 message (PC → phone) |

Sequence: `(FILE DATA* FILE_END | TEXT)* END`, then the PC replies OK or ERR. An empty sequence (`END` alone)
is a connection check. A file not closed by FILE_END is deleted; names are reduced to their last path
component and stripped of characters Windows rejects.

## v2: which PC, PC to phone, PC to PC

All additions are ignored by v1 peers: a v1 PC never answers `DCD2`, and the phone only sends the
new records to a PC that has shown it speaks v2 (an answer to `DCD2`, or the v2 flag in `OK`).

**Peer key.** `peer_key(K, from) = HMAC(K, "dropcopy/v2/peer" | from[8])`. A device that holds the
pairing key itself (a phone) uses `from` = 8 zero bytes and K. Another PC uses its own id as `from`
and the peer key a phone gave it. `Kf` below is "K for a phone, the peer key for a PC".

### Which PC? — UDP 48651

```
phone → PC/broadcast "DCD2" | id[8] | from[8] | nonce[16] | HMAC(Kf, "dropcopy/v2/ask" | id | from | nonce)[0..16]   (52 bytes)
PC    → phone        "DCR2" | id[8] | n[12] | AES-256-GCM(HMAC(Kf, "dropcopy/v2/disc"), n,
                              aad = "DCR2" | id | from | nonce, body[8]) | tag[16]                             (48 bytes)
                     body = port u16 | idle s u16 (65535 unknown) | phone's BLE dBm i16 (−32768 none) | flags u16
                     flags: 0x0001 something waits here for a phone · 0x0080 v2 · 0x0100 the phone is close (BLE)
```

The request is authenticated (a PC ignores strangers) and the answer encrypted, so nobody on the
network learns whether you are at your PC. With several PCs paired and none chosen, the phone asks
them all and waits up to 300 ms (less once all answered), then sends to the PC it is held next to,
else the one used most recently (idle under 120 s, smallest first), else the last one it used; no
answer at all falls back to the v1 race.

### PC to phone

The PC keeps what you send to a phone in its outbox (`outbox\` in the settings folder) and the phone
collects it; a phone can't be connected to at any moment, so it always dials out.

```
PC → phone (its hello address) "DCO1" | id[8] | nonce[16] | HMAC(K, "dropcopy/v2/offer" | id | nonce)[0..16]   (44 bytes)
```

The phone listens on the socket it says hello from (every 15 s while its screen is on, with
*Receive from your PCs* on). On an offer, or an `OK` with flag 0x01, or a `DCR2` with flag 0x0001,
or when the app opens, it connects as usual and sends `DEVICE`, `FETCH`, `END`. The PC answers with
the items (`FILE DATA* FILE_END | TEXT`)* under k_s2c, then `OK`. The phone confirms with an `OK`
record of its own; only then does the PC delete them, so a broken connection loses nothing.

### PC to PC

A phone paired with several v2 PCs sends each one, in any transfer, a `PEERS` record: for every
other PC, `len u16 | dropcopy://peer?v=2&id=<other>&k=<peer_key(K_other, this id)>&n=<name>&p=<port>&a=<ip>`.
A PC never receives another PC's pairing key, so it can't pose as that PC to phones, and a new key
on either PC (`dropcopy pair --new`) ends the link. To send, a PC connects with

```
C → S   "DCP2" | cn[16] | from[8]          then the v1 handshake, keyed by peer_key(K_S, from)
```

and sends `DEVICE` (its name), the items and `END`, exactly like a phone.

| type | name | body |
|---|---|---|
| 0x08 | FETCH | — (phone → PC) after END, send me what waits for me |
| 0x09 | PEERS | (len u16 \| peer link)* (phone → PC) |
| 0x10 | OK | count u16 \| flags u8 (v2: 0x01 something waits for you, 0x80 v2) |

### Local notification (same machine only)

```
local → 127.0.0.1   "DCT1" | title | 0x00 | text      (at most 512 bytes)
```

`dropcopy share` (Explorer's Send to) asks the background copy to show its result from the tray icon.
