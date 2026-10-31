# DropCopy protocol v1

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
| 0x10 | OK | count u16 (PC → phone) |
| 0x11 | ERR | UTF-8 message (PC → phone) |

Sequence: `(FILE DATA* FILE_END | TEXT)* END`, then the PC replies OK or ERR. An empty sequence (`END` alone)
is a connection check. A file not closed by FILE_END is deleted; names are reduced to their last path
component and stripped of characters Windows rejects.
