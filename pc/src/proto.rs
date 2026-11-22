//! Wire protocol: handshake, authenticated-encryption records, discovery packets.
//!
//! Handshake (1 round trip after TCP connect):
//!   C -> S  "DCP1" | client_nonce[16]
//!   S -> C  "DCP1" | server_nonce[16] | HMAC(K, "dropcopy/v1/srv" | cn | sn)
//! Both sides then derive one AES-256-GCM key per direction from K and both nonces.
//!
//! Record: u32 BE length | AES-256-GCM(type[1] | body), nonce = 0u32 | counter u64 BE.

use aes_gcm::aead::{AeadInPlace, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce, Tag};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::io::{self, Read, Write};

pub const MAGIC: &[u8; 4] = b"DCP1";
pub const DISCOVER_MAGIC: &[u8; 4] = b"DCD1";
pub const REPLY_MAGIC: &[u8; 4] = b"DCR1";
/// An unpaired phone asking "is a DropCopy PC here?" so the PC can put its pairing code on screen.
pub const PAIR_ASK_MAGIC: &[u8; 4] = b"DCQ1";
pub const PAIR_ANSWER_MAGIC: &[u8; 4] = b"DCA1";
/// The phone asking "do you see me over Bluetooth?" while the user sets proximity up.
pub const NEAR_ASK_MAGIC: &[u8; 4] = b"DCN1";
pub const NEAR_ANSWER_MAGIC: &[u8; 4] = b"DCM1";
const L_NEARQ: &[u8] = b"dropcopy/v1/nearq";
/// The phone, while proximity is on and its screen is on, telling the PC where to reach it, and
/// the PC asking it to send its newest photo because it just came close.
pub const HELLO_MAGIC: &[u8; 4] = b"DCH1";
pub const PULL_MAGIC: &[u8; 4] = b"DCU1";
const L_HELLO: &[u8] = b"dropcopy/v1/hello";
const L_PULL: &[u8] = b"dropcopy/v1/pull";
/// v2 discovery: an authenticated "where are you, and is someone using you?" and its encrypted
/// answer, so a phone with several PCs can pick the one in use. Also used PC to PC.
pub const ASK_MAGIC: &[u8; 4] = b"DCD2";
pub const STATUS_MAGIC: &[u8; 4] = b"DCR2";
const L_ASK: &[u8] = b"dropcopy/v2/ask";
const L_STATUS: &[u8] = b"dropcopy/v2/disc";
/// The PC telling a phone "something is waiting for you here: come and fetch it".
pub const OFFER_MAGIC: &[u8; 4] = b"DCO1";
const L_OFFER: &[u8] = b"dropcopy/v2/offer";
/// Handshake of another PC, introduced by a phone: `"DCP2" | cn[16] | from[8]`, keyed by the peer key.
pub const PEER_MAGIC: &[u8; 4] = b"DCP2";
const L_PEER: &[u8] = b"dropcopy/v2/peer";
/// From this machine only: `dropcopy share` asking the background copy to show a notification
/// from its tray icon: "DCT1" | title | 0 | text.
pub const TOAST_MAGIC: &[u8; 4] = b"DCT1";
pub const TOAST_MAX: usize = 512;
/// From this machine only: a newer copy asking the running one to quit so it can take over.
pub const STOP_MAGIC: &[u8; 4] = b"DCS1";
pub const TCP_PORT: u16 = 48650;
pub const UDP_PORT: u16 = 48651;

/// Largest plaintext a record may carry (type byte included).
pub const MAX_RECORD: usize = 1 << 20;
/// File data is sent in chunks of this size.
pub const CHUNK: usize = 512 * 1024;
const TAG_LEN: usize = 16;

pub const T_FILE: u8 = 0x01; // size u64 | name_len u16 | name | mime_len u16 | mime
pub const T_DATA: u8 = 0x02; // raw bytes of the current file
pub const T_FILE_END: u8 = 0x03; // current file is complete
pub const T_TEXT: u8 = 0x04; // utf-8 text for the clipboard
pub const T_END: u8 = 0x05; // sender is done
pub const T_OPTS: u8 = 0x06; // flags u8, before any file: how to treat this transfer
pub const T_DEVICE: u8 = 0x07; // utf-8, at most 64 bytes: the phone's own name ("Galaxy S20+")
pub const T_FETCH: u8 = 0x08; // (v2) after END, send me what is waiting for me
pub const T_PEERS: u8 = 0x09; // (v2) other PCs this phone is paired with: (len u16 | peer link)*
pub const T_PHONE: u8 = 0x0A; // (v2) the phone's install id (ASCII hex), which tells apart phones of the same name
/// `T_OPTS` flags.
pub const OPT_NEARBY: u8 = 0x01; // a proximity preview: temporary, shown on the taskbar phone only
pub const OPT_DELETE_AFTER_PASTE: u8 = 0x02; // remove the file once pasted (or the clipboard moves on)
pub const T_OK: u8 = 0x10; // count u16 | (v2) flags u8
/// Flags after the count in `T_OK` (v2), and in the status answer.
pub const OK_WAITING: u8 = 0x01; // something is waiting on this PC for the device that asked
pub const OK_V2: u8 = 0x80; // this PC speaks v2 (fetch, peers)
pub const T_ERR: u8 = 0x11; // utf-8 message

const L_SRV: &[u8] = b"dropcopy/v1/srv";
const L_C2S: &[u8] = b"dropcopy/v1/c2s";
const L_S2C: &[u8] = b"dropcopy/v1/s2c";
const L_DISC: &[u8] = b"dropcopy/v1/disc";

pub fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("hmac accepts any key length");
    for p in parts {
        m.update(p);
    }
    m.finalize().into_bytes().into()
}

pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).expect("OS random source");
    b
}

fn bad(msg: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg.to_string())
}

/// One direction of the encrypted record stream.
pub struct Records {
    cipher: Aes256Gcm,
    ctr: u64,
}

impl Records {
    pub fn new(key: &[u8; 32]) -> Self {
        Records { cipher: Aes256Gcm::new(key.into()), ctr: 0 }
    }

    fn next_nonce(&mut self) -> [u8; 12] {
        let mut n = [0u8; 12];
        n[4..].copy_from_slice(&self.ctr.to_be_bytes());
        self.ctr += 1;
        n
    }

    /// Encrypts and writes one record. `buf` is scratch space reused between calls.
    pub fn write<W: Write>(&mut self, w: &mut W, typ: u8, body: &[u8], buf: &mut Vec<u8>) -> io::Result<()> {
        if body.len() + 1 > MAX_RECORD {
            return Err(bad("record too large"));
        }
        buf.clear();
        buf.extend_from_slice(&[0u8; 4]);
        buf.push(typ);
        buf.extend_from_slice(body);
        let nonce = self.next_nonce();
        let tag = self
            .cipher
            .encrypt_in_place_detached(Nonce::from_slice(&nonce), b"", &mut buf[4..])
            .map_err(|_| bad("encrypt failed"))?;
        buf.extend_from_slice(&tag);
        let len = (buf.len() - 4) as u32;
        buf[..4].copy_from_slice(&len.to_be_bytes());
        w.write_all(buf)
    }

    /// Reads and decrypts one record into `buf` (type byte first).
    /// Returns `None` on a clean end of stream between records.
    pub fn read<R: Read>(&mut self, r: &mut R, buf: &mut Vec<u8>) -> io::Result<Option<u8>> {
        let mut len = [0u8; 4];
        match r.read_exact(&mut len) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e),
        }
        let len = u32::from_be_bytes(len) as usize;
        if !(1 + TAG_LEN..=MAX_RECORD + TAG_LEN).contains(&len) {
            return Err(bad("bad record length"));
        }
        buf.resize(len, 0);
        r.read_exact(buf)?;
        let body = len - TAG_LEN;
        let tag = *Tag::from_slice(&buf[body..]);
        let nonce = self.next_nonce();
        self.cipher
            .decrypt_in_place_detached(Nonce::from_slice(&nonce), b"", &mut buf[..body], &tag)
            .map_err(|_| bad("record failed authentication (phone not paired with this PC?)"))?;
        buf.truncate(body);
        Ok(Some(buf[0]))
    }
}

fn derive(key: &[u8; 32], cn: &[u8], sn: &[u8]) -> ([u8; 32], [u8; 32]) {
    (hmac(key, &[L_C2S, cn, sn]), hmac(key, &[L_S2C, cn, sn]))
}

/// The key another PC (`from`) uses with the PC whose key is `key`. A phone paired with both hands
/// it over (it knows `key`); the receiving PC derives it again from `from` in the handshake. The
/// other PC never learns `key` itself, so it cannot pose as this PC to phones, and a new pairing
/// key (`dropcopy pair --new`) cuts it off too.
pub fn peer_key(key: &[u8; 32], from: &[u8; 8]) -> [u8; 32] {
    hmac(key, &[L_PEER, from])
}

/// Server side. Returns (records from client, records to client).
pub fn server_handshake<R: Read, W: Write>(r: &mut R, w: &mut W, key: &[u8; 32]) -> io::Result<(Records, Records)> {
    server_handshake_any(r, w, key).map(|(rx, tx, _)| (rx, tx))
}

/// Server side, for phones (`DCP1`, the pairing key) and introduced PCs (`DCP2`, their peer key).
/// The third value is the other PC's id when it was a PC.
pub fn server_handshake_any<R: Read, W: Write>(r: &mut R, w: &mut W, key: &[u8; 32]) -> io::Result<(Records, Records, Option<[u8; 8]>)> {
    let mut hello = [0u8; 20];
    r.read_exact(&mut hello)?;
    let (k, from) = if &hello[..4] == MAGIC {
        (*key, None)
    } else if &hello[..4] == PEER_MAGIC {
        let mut from = [0u8; 8];
        r.read_exact(&mut from)?;
        (peer_key(key, &from), Some(from))
    } else {
        return Err(bad("not a DropCopy client"));
    };
    let cn = &hello[4..20];
    let sn: [u8; 16] = random();
    let proof = hmac(&k, &[L_SRV, cn, &sn]);
    let mut out = Vec::with_capacity(52);
    out.extend_from_slice(&hello[..4]);
    out.extend_from_slice(&sn);
    out.extend_from_slice(&proof);
    w.write_all(&out)?;
    w.flush()?;
    let (c2s, s2c) = derive(&k, cn, &sn);
    Ok((Records::new(&c2s), Records::new(&s2c), from))
}

/// Client side. Verifies the server knows the key. Returns (records to server, records from server).
pub fn client_handshake<R: Read, W: Write>(r: &mut R, w: &mut W, key: &[u8; 32]) -> io::Result<(Records, Records)> {
    client_handshake_as(r, w, key, None)
}

/// Client side as a PC introduced by a phone: `key` is the peer key, `from` this PC's id.
pub fn client_handshake_as<R: Read, W: Write>(r: &mut R, w: &mut W, key: &[u8; 32], from: Option<&[u8; 8]>) -> io::Result<(Records, Records)> {
    let cn: [u8; 16] = random();
    let magic = if from.is_some() { PEER_MAGIC } else { MAGIC };
    let mut hello = Vec::with_capacity(28);
    hello.extend_from_slice(magic);
    hello.extend_from_slice(&cn);
    if let Some(f) = from {
        hello.extend_from_slice(f);
    }
    w.write_all(&hello)?;
    w.flush()?;
    let mut resp = [0u8; 52];
    r.read_exact(&mut resp)?;
    if &resp[..4] != magic {
        return Err(bad("not a DropCopy receiver"));
    }
    let sn = &resp[4..20];
    if !ct_eq(&resp[20..52], &hmac(key, &[L_SRV, &cn, sn])) {
        return Err(bad("receiver failed authentication (different pairing key)"));
    }
    let (c2s, s2c) = derive(key, &cn, sn);
    Ok((Records::new(&c2s), Records::new(&s2c)))
}

/// What a PC tells a paired device that asks (v2 discovery).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Status {
    pub port: u16,
    /// Seconds since someone last used this PC's keyboard or mouse (`u16::MAX`: unknown, or longer).
    pub idle_s: u16,
    /// The asking phone's Bluetooth signal here, smoothed (`i16::MIN`: not heard lately).
    pub near_dbm: i16,
    /// `OK_WAITING`: something is waiting here for the device that asked. `OK_V2` always.
    /// `STATUS_CLOSE`: the phone is close by its Bluetooth signal.
    pub flags: u16,
}

pub const STATUS_CLOSE: u16 = 0x0100;

/// The key a request from `from` uses: the pairing key itself for a phone (all zeros), else the peer key.
pub fn key_for(key: &[u8; 32], from: &[u8; 8]) -> [u8; 32] {
    if from == &[0u8; 8] {
        *key
    } else {
        peer_key(key, from)
    }
}

/// `"DCD2" | id[8] | from[8] | nonce[16] | HMAC(Kf, "dropcopy/v2/ask" | id | from | nonce)[..16]` (52 bytes).
/// `from` is all zeros for a phone, which holds the pairing key itself.
pub fn ask_packet(id: &[u8; 8], kf: &[u8; 32], from: &[u8; 8], nonce: &[u8; 16]) -> Vec<u8> {
    [ASK_MAGIC.as_slice(), id, from, nonce, &hmac(kf, &[L_ASK, id, from, nonce])[..16]].concat()
}

/// If `pkt` is a genuine v2 discovery request for this PC, returns (from, nonce, key to answer with).
pub fn parse_ask(pkt: &[u8], id: &[u8; 8], key: &[u8; 32]) -> Option<([u8; 8], [u8; 16], [u8; 32])> {
    if pkt.len() != 52 || &pkt[..4] != ASK_MAGIC || &pkt[4..12] != id {
        return None;
    }
    let from: [u8; 8] = pkt[12..20].try_into().ok()?;
    let nonce: [u8; 16] = pkt[20..36].try_into().ok()?;
    let kf = key_for(key, &from);
    ct_eq(&pkt[36..52], &hmac(&kf, &[L_ASK, id, &from, &nonce])[..16]).then_some((from, nonce, kf))
}

fn status_cipher(kf: &[u8; 32]) -> Aes256Gcm {
    Aes256Gcm::new((&hmac(kf, &[L_STATUS])).into())
}

/// `"DCR2" | id[8] | n[12] | AES-256-GCM(body[8]) | tag[16]` (48 bytes), additional data
/// `"DCR2" | id | from | request nonce`. Encrypted, so the network can't tell whether you are at your PC.
pub fn status_packet(id: &[u8; 8], kf: &[u8; 32], from: &[u8; 8], nonce: &[u8; 16], st: &Status) -> Vec<u8> {
    let n: [u8; 12] = random();
    let mut body = Vec::with_capacity(8);
    body.extend_from_slice(&st.port.to_be_bytes());
    body.extend_from_slice(&st.idle_s.to_be_bytes());
    body.extend_from_slice(&st.near_dbm.to_be_bytes());
    body.extend_from_slice(&st.flags.to_be_bytes());
    let aad = [STATUS_MAGIC.as_slice(), id, from, nonce].concat();
    let tag = status_cipher(kf).encrypt_in_place_detached(Nonce::from_slice(&n), &aad, &mut body).expect("8 bytes always encrypt");
    [STATUS_MAGIC.as_slice(), id, &n, &body, &tag].concat()
}

pub fn parse_status(pkt: &[u8], id: &[u8; 8], kf: &[u8; 32], from: &[u8; 8], nonce: &[u8; 16]) -> Option<Status> {
    if pkt.len() != 48 || &pkt[..4] != STATUS_MAGIC || &pkt[4..12] != id {
        return None;
    }
    let mut body = pkt[24..32].to_vec();
    let tag = *Tag::from_slice(&pkt[32..48]);
    let aad = [STATUS_MAGIC.as_slice(), id, from, nonce].concat();
    status_cipher(kf).decrypt_in_place_detached(Nonce::from_slice(&pkt[12..24]), &aad, &mut body, &tag).ok()?;
    let u = |i: usize| u16::from_be_bytes([body[i], body[i + 1]]);
    Some(Status { port: u(0), idle_s: u(2), near_dbm: u(4) as i16, flags: u(6) })
}

/// "DCO1" | id[8] | nonce[16] | HMAC(K, "dropcopy/v2/offer" | id | nonce)[..16]  (44 bytes)
pub fn offer_packet(id: &[u8; 8], key: &[u8; 32], nonce: &[u8; 16]) -> Vec<u8> {
    [OFFER_MAGIC.as_slice(), id, nonce, &hmac(key, &[L_OFFER, id, nonce])[..16]].concat()
}

pub fn is_offer(pkt: &[u8], id: &[u8; 8], key: &[u8; 32]) -> bool {
    pkt.len() == 44 && &pkt[..4] == OFFER_MAGIC && &pkt[4..12] == id && ct_eq(&pkt[28..44], &hmac(key, &[L_OFFER, id, &pkt[12..28]])[..16])
}

/// "DCD1" | pc_id[8] | nonce[16]
pub fn discover_packet(id: &[u8; 8], nonce: &[u8; 16]) -> [u8; 28] {
    let mut p = [0u8; 28];
    p[..4].copy_from_slice(DISCOVER_MAGIC);
    p[4..12].copy_from_slice(id);
    p[12..].copy_from_slice(nonce);
    p
}

/// "DCR1" | pc_id[8] | tcp_port u16 | HMAC(K, "dropcopy/v1/disc" | nonce | port)[..16]
pub fn reply_packet(id: &[u8; 8], key: &[u8; 32], nonce: &[u8], port: u16) -> [u8; 30] {
    let mut p = [0u8; 30];
    p[..4].copy_from_slice(REPLY_MAGIC);
    p[4..12].copy_from_slice(id);
    p[12..14].copy_from_slice(&port.to_be_bytes());
    p[14..].copy_from_slice(&hmac(key, &[L_DISC, nonce, &port.to_be_bytes()])[..16]);
    p
}

/// "DCA1" | nonce[8] | name (utf-8, at most 64 bytes). Carries no secret: the key only ever
/// travels in the QR code, which the phone reads off the PC's screen.
pub fn pair_answer(nonce: &[u8], name: &str) -> Vec<u8> {
    let mut end = name.len().min(64);
    while !name.is_char_boundary(end) {
        end -= 1;
    }
    let mut p = Vec::with_capacity(12 + end);
    p.extend_from_slice(PAIR_ANSWER_MAGIC);
    p.extend_from_slice(&nonce[..8]);
    p.extend_from_slice(&name.as_bytes()[..end]);
    p
}

/// "DCM1" | id[8] | body[7] | HMAC(K, "dropcopy/v1/nearq" | nonce | body)[..16], where body is
/// state u8 | smoothed dBm i16 (i16::MIN = not heard in the last 5 s) | threshold i16 | age ms u16.
pub fn near_answer(id: &[u8; 8], key: &[u8; 32], nonce: &[u8], st: &crate::near::Status) -> Vec<u8> {
    let fresh = st.last.filter(|(_, age)| age.as_secs() < 5);
    let dbm = fresh.map(|(s, _)| s.round() as i16).unwrap_or(i16::MIN);
    let age = fresh.map(|(_, a)| a.as_millis().min(65_535) as u16).unwrap_or(u16::MAX);
    let mut body = vec![st.state];
    body.extend_from_slice(&dbm.to_be_bytes());
    body.extend_from_slice(&st.threshold.to_be_bytes());
    body.extend_from_slice(&age.to_be_bytes());
    let mut p = Vec::with_capacity(35);
    p.extend_from_slice(NEAR_ANSWER_MAGIC);
    p.extend_from_slice(id);
    p.extend_from_slice(&body);
    p.extend_from_slice(&hmac(key, &[L_NEARQ, nonce, &body])[..16]);
    p
}

/// "DCH1" | id[8] | nonce[16] | HMAC(K, "dropcopy/v1/hello" | nonce)[..16]  (44 bytes)
pub fn is_hello(pkt: &[u8], id: &[u8; 8], key: &[u8; 32]) -> bool {
    pkt.len() == 44
        && &pkt[..4] == HELLO_MAGIC
        && &pkt[4..12] == id
        && ct_eq(&pkt[28..44], &hmac(key, &[L_HELLO, &pkt[12..28]])[..16])
}

pub fn hello_packet(id: &[u8; 8], key: &[u8; 32], nonce: &[u8; 16]) -> Vec<u8> {
    [HELLO_MAGIC.as_slice(), id, nonce, &hmac(key, &[L_HELLO, nonce])[..16]].concat()
}

/// "DCU1" | id[8] | nonce[16] | HMAC(K, "dropcopy/v1/pull" | nonce)[..16]  (44 bytes)
pub fn pull_packet(id: &[u8; 8], key: &[u8; 32], nonce: &[u8; 16]) -> Vec<u8> {
    [PULL_MAGIC.as_slice(), id, nonce, &hmac(key, &[L_PULL, nonce])[..16]].concat()
}

/// Returns the TCP port if `pkt` is a genuine reply to `nonce` from the PC `id`.
pub fn parse_reply(pkt: &[u8], id: &[u8; 8], key: &[u8; 32], nonce: &[u8; 16]) -> Option<u16> {
    if pkt.len() != 30 || &pkt[..4] != REPLY_MAGIC || &pkt[4..12] != id {
        return None;
    }
    let port = u16::from_be_bytes([pkt[12], pkt[13]]);
    let want = hmac(key, &[L_DISC, nonce, &port.to_be_bytes()]);
    ct_eq(&pkt[14..30], &want[..16]).then_some(port)
}

/// Builds a T_FILE body.
pub fn file_header(size: u64, name: &str, mime: &str) -> Vec<u8> {
    let name = &name.as_bytes()[..name.len().min(u16::MAX as usize)];
    let mime = &mime.as_bytes()[..mime.len().min(u16::MAX as usize)];
    let mut b = Vec::with_capacity(12 + name.len() + mime.len());
    b.extend_from_slice(&size.to_be_bytes());
    b.extend_from_slice(&(name.len() as u16).to_be_bytes());
    b.extend_from_slice(name);
    b.extend_from_slice(&(mime.len() as u16).to_be_bytes());
    b.extend_from_slice(mime);
    b
}

pub struct FileHeader {
    pub size: u64,
    pub name: String,
    pub mime: String,
}

pub fn parse_file_header(b: &[u8]) -> io::Result<FileHeader> {
    let err = || bad("bad file header");
    let size = u64::from_be_bytes(b.get(..8).ok_or_else(err)?.try_into().unwrap());
    let mut at = 8;
    let mut take_str = || -> io::Result<String> {
        let n = u16::from_be_bytes(b.get(at..at + 2).ok_or_else(err)?.try_into().unwrap()) as usize;
        let s = b.get(at + 2..at + 2 + n).ok_or_else(err)?;
        at += 2 + n;
        Ok(String::from_utf8_lossy(s).into_owned())
    };
    let name = take_str()?;
    let mime = take_str()?;
    Ok(FileHeader { size, name, mime })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handshake_and_records_roundtrip() {
        use std::net::{TcpListener, TcpStream};
        let key: [u8; 32] = random();
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let srv = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut s2 = s.try_clone().unwrap();
            let (mut rx, mut tx) = server_handshake(&mut s, &mut s2, &key).unwrap();
            let mut buf = Vec::new();
            assert_eq!(rx.read(&mut s, &mut buf).unwrap(), Some(T_TEXT));
            assert_eq!(&buf[1..], b"hello");
            assert_eq!(rx.read(&mut s, &mut buf).unwrap(), Some(T_END));
            tx.write(&mut s2, T_OK, &1u16.to_be_bytes(), &mut buf).unwrap();
        });
        let mut c = TcpStream::connect(addr).unwrap();
        let mut c2 = c.try_clone().unwrap();
        let (mut tx, mut rx) = client_handshake(&mut c, &mut c2, &key).unwrap();
        let mut buf = Vec::new();
        tx.write(&mut c2, T_TEXT, b"hello", &mut buf).unwrap();
        tx.write(&mut c2, T_END, b"", &mut buf).unwrap();
        assert_eq!(rx.read(&mut c, &mut buf).unwrap(), Some(T_OK));
        srv.join().unwrap();

        // A client holding a different key must refuse the server.
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut s2 = s.try_clone().unwrap();
            let _ = server_handshake(&mut s, &mut s2, &key);
        });
        let mut c = TcpStream::connect(addr).unwrap();
        let mut c2 = c.try_clone().unwrap();
        assert!(client_handshake(&mut c, &mut c2, &[0u8; 32]).is_err());
    }

    #[test]
    fn tampered_record_is_rejected() {
        let key = [1u8; 32];
        let mut w = Records::new(&key);
        let mut r = Records::new(&key);
        let mut wire = Vec::new();
        let mut buf = Vec::new();
        w.write(&mut wire, T_TEXT, b"secret", &mut buf).unwrap();
        wire[6] ^= 1;
        assert!(r.read(&mut &wire[..], &mut buf).is_err());
    }

    #[test]
    fn pair_answer_carries_only_the_name_cut_on_a_char_boundary() {
        let a = pair_answer(&[9; 8], "PC");
        assert_eq!(a, b"DCA1\x09\x09\x09\x09\x09\x09\x09\x09PC");
        let long = "מחשב".repeat(20); // 160 bytes of 2-byte chars
        let a = pair_answer(&[0; 8], &long);
        assert!(a.len() <= 12 + 64);
        assert!(std::str::from_utf8(&a[12..]).is_ok());
    }

    #[test]
    fn discovery_reply_verifies() {
        let key = [3u8; 32];
        let id = [9u8; 8];
        let nonce = [5u8; 16];
        let p = reply_packet(&id, &key, &nonce, 48650);
        assert_eq!(parse_reply(&p, &id, &key, &nonce), Some(48650));
        assert_eq!(parse_reply(&p, &id, &[4u8; 32], &nonce), None);
        assert_eq!(parse_reply(&p, &id, &key, &[6u8; 16]), None);
    }

    #[test]
    fn status_is_authenticated_and_encrypted() {
        let key = [3u8; 32];
        let id = [9u8; 8];
        let nonce = [5u8; 16];
        let phone = [0u8; 8];
        let ask = ask_packet(&id, &key, &phone, &nonce);
        let (from, n, kf) = parse_ask(&ask, &id, &key).expect("phone's ask verifies");
        assert_eq!((from, n, kf), (phone, nonce, key));
        assert!(parse_ask(&ask_packet(&id, &[4u8; 32], &phone, &nonce), &id, &key).is_none(), "wrong key");
        let st = Status { port: 48650, idle_s: 7, near_dbm: -48, flags: OK_V2 as u16 | STATUS_CLOSE };
        let p = status_packet(&id, &kf, &from, &nonce, &st);
        assert_eq!(p.len(), 48);
        assert!(!p.windows(2).any(|w| w == 48650u16.to_be_bytes()), "the port is not in the clear");
        assert_eq!(parse_status(&p, &id, &key, &phone, &nonce), Some(st));
        assert_eq!(parse_status(&p, &id, &key, &phone, &[6u8; 16]), None, "answer to another question");
        let mut t = p.clone();
        t[26] ^= 1;
        assert_eq!(parse_status(&t, &id, &key, &phone, &nonce), None, "tampered");

        // Another PC asks with its peer key; only that key reads the answer.
        let pc = [7u8; 8];
        let pk = peer_key(&key, &pc);
        let ask = ask_packet(&id, &pk, &pc, &nonce);
        let (from, _, kf) = parse_ask(&ask, &id, &key).expect("peer's ask verifies");
        assert_eq!((from, kf), (pc, pk));
        let p = status_packet(&id, &kf, &from, &nonce, &st);
        assert_eq!(parse_status(&p, &id, &pk, &pc, &nonce), Some(st));
        assert_eq!(parse_status(&p, &id, &key, &pc, &nonce), None);
    }

    #[test]
    fn offer_verifies() {
        let (id, key) = ([1u8; 8], [2u8; 32]);
        let p = offer_packet(&id, &key, &[3u8; 16]);
        assert!(is_offer(&p, &id, &key));
        assert!(!is_offer(&p, &id, &[9u8; 32]));
        assert!(!is_offer(&p, &[8u8; 8], &key));
    }

    #[test]
    fn peer_handshake_uses_the_peer_key() {
        use std::net::{TcpListener, TcpStream};
        let key: [u8; 32] = random();
        let me = [4u8; 8];
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let srv = std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut s2 = s.try_clone().unwrap();
            let (mut rx, _, from) = server_handshake_any(&mut s, &mut s2, &key).unwrap();
            let mut buf = Vec::new();
            assert_eq!(rx.read(&mut s, &mut buf).unwrap(), Some(T_END));
            from
        });
        let mut c = TcpStream::connect(addr).unwrap();
        let mut c2 = c.try_clone().unwrap();
        let (mut tx, _) = client_handshake_as(&mut c, &mut c2, &peer_key(&key, &me), Some(&me)).unwrap();
        tx.write(&mut c2, T_END, b"", &mut Vec::new()).unwrap();
        assert_eq!(srv.join().unwrap(), Some(me));

        // The pairing key itself does not work as a peer, nor a peer key for another id.
        for (k, from) in [(key, me), (peer_key(&key, &[5u8; 8]), me)] {
            let l = TcpListener::bind("127.0.0.1:0").unwrap();
            let addr = l.local_addr().unwrap();
            std::thread::spawn(move || {
                let (mut s, _) = l.accept().unwrap();
                let mut s2 = s.try_clone().unwrap();
                let _ = server_handshake_any(&mut s, &mut s2, &key);
            });
            let mut c = TcpStream::connect(addr).unwrap();
            let mut c2 = c.try_clone().unwrap();
            assert!(client_handshake_as(&mut c, &mut c2, &k, Some(&from)).is_err());
        }
    }

    #[test]
    fn file_header_roundtrip() {
        let h = parse_file_header(&file_header(42, "IMG_1.jpg", "image/jpeg")).unwrap();
        assert_eq!((h.size, h.name.as_str(), h.mime.as_str()), (42, "IMG_1.jpg", "image/jpeg"));
        assert!(parse_file_header(&[0u8; 9]).is_err());
    }
}
