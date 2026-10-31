//! "Bring the phone close": the phone broadcasts a tiny Bluetooth LE advertisement, the PC watches
//! its signal strength, and when the phone is held near the laptop the latest photo pops up.
//!
//! The advertisement carries manufacturer data under company id 0xFFFF (reserved for testing,
//! never assigned to a vendor): `"DC"` | token[8]. The token is HMAC(K, "dropcopy/v1/near" |
//! minute) and changes every minute, so it identifies the phone only to its own PC and cannot be
//! used to track it.

use crate::proto::hmac;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const COMPANY_ID: u16 = 0xFFFF;
pub const PREFIX: &[u8; 2] = b"DC";
const L_NEAR: &[u8] = b"dropcopy/v1/near";

pub fn minute_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() / 60).unwrap_or(0)
}

pub fn token(key: &[u8; 32], minute: u64) -> [u8; 8] {
    let h = hmac(key, &[L_NEAR, &minute.to_be_bytes()]);
    h[..8].try_into().unwrap()
}

/// True if `data` (the manufacturer-data payload) is this PC's phone, allowing a minute of clock skew.
pub fn matches(key: &[u8; 32], data: &[u8], minute: u64) -> bool {
    if data.len() != 10 || &data[..2] != PREFIX {
        return false;
    }
    (minute.saturating_sub(1)..=minute + 1).any(|m| crate::proto::ct_eq(&data[2..], &token(key, m)))
}

/// Turns a noisy stream of RSSI readings into "the phone was just brought close" events.
///
/// Bluetooth signal strength jumps by ±6 dB between packets, so readings are smoothed, and entering
/// and leaving use different thresholds (hysteresis) so a phone resting at the boundary doesn't
/// flicker. After an event it stays quiet until the phone has been moved away again, or for
/// `cooldown`.
pub struct Detector {
    pub enter_dbm: f32,
    pub leave_dbm: f32,
    pub cooldown: Duration,
    avg: Option<f32>,
    samples: u32,
    last_seen: Option<Instant>,
    near: bool,
    last_event: Option<Instant>,
}

impl Detector {
    pub fn new(enter_dbm: i16) -> Detector {
        Detector {
            enter_dbm: enter_dbm as f32,
            leave_dbm: enter_dbm as f32 - 12.0,
            cooldown: Duration::from_secs(20),
            avg: None,
            samples: 0,
            last_seen: None,
            near: false,
            last_event: None,
        }
    }

    /// Feeds one reading; returns true when the phone has just come near.
    pub fn feed(&mut self, rssi: i16, now: Instant) -> bool {
        // A gap of more than a few seconds means the phone left range: start fresh.
        if self.last_seen.is_some_and(|t| now.duration_since(t) > Duration::from_secs(4)) {
            self.avg = None;
            self.samples = 0;
            self.near = false;
        }
        self.last_seen = Some(now);
        let r = rssi as f32;
        let avg = match self.avg {
            None => r,
            // Rising signal is trusted faster than falling, so "brought close" reacts quickly.
            Some(a) if r > a => a + 0.5 * (r - a),
            Some(a) => a + 0.25 * (r - a),
        };
        self.avg = Some(avg);
        self.samples += 1;

        if self.near {
            if avg < self.leave_dbm {
                self.near = false;
            }
            return false;
        }
        if self.samples >= 2 && avg >= self.enter_dbm {
            self.near = true;
            let cooled = self.last_event.is_none_or(|t| now.duration_since(t) >= self.cooldown);
            if cooled {
                self.last_event = Some(now);
                return true;
            }
        }
        false
    }

    pub fn smoothed(&self) -> Option<f32> {
        self.avg
    }
}

/// One authentic reading from the phone: raw and smoothed strength, and whether it just came near.
pub struct Reading {
    pub rssi: i16,
    pub smoothed: f32,
    pub came_near: bool,
}

/// What the Bluetooth watcher knows right now, for the phone's "does my PC see me?" check.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Status {
    pub state: u8,
    /// Smoothed strength and how long ago it was heard.
    pub last: Option<(f32, Duration)>,
    pub threshold: i16,
}

/// `Status::state`: not watching (turned off), Bluetooth unavailable, scanning.
pub const OFF: u8 = 0;
pub const NO_BLUETOOTH: u8 = 1;
pub const SCANNING: u8 = 2;

static STATE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(OFF);
static THRESHOLD: std::sync::atomic::AtomicI16 = std::sync::atomic::AtomicI16::new(-55);
static LAST: std::sync::Mutex<Option<(f32, Instant)>> = std::sync::Mutex::new(None);

/// Where the phone said it can be reached, and when.
static PHONE: std::sync::Mutex<Option<(std::net::SocketAddr, Instant)>> = std::sync::Mutex::new(None);

/// The last time the phone was heard from at all: a hello, a check-in or a transfer.
static CONTACT: std::sync::Mutex<Option<Instant>> = std::sync::Mutex::new(None);

pub fn heard_from_phone() {
    *CONTACT.lock().unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Link {
    /// Heard within the last 40 s (the phone says hello every 15 s while its screen is on).
    Connected,
    /// Heard recently, but hellos have been missed: weak Wi-Fi, or the phone is going to sleep.
    Unstable,
    Offline,
}

pub fn link_after(since: Option<Duration>) -> Link {
    match since {
        Some(d) if d < Duration::from_secs(40) => Link::Connected,
        Some(d) if d < Duration::from_secs(180) => Link::Unstable,
        _ => Link::Offline,
    }
}

pub fn link() -> Link {
    link_after(CONTACT.lock().unwrap_or_else(|e| e.into_inner()).map(|t| t.elapsed()))
}

pub fn phone_is_at(addr: std::net::SocketAddr) {
    heard_from_phone();
    *PHONE.lock().unwrap_or_else(|e| e.into_inner()) = Some((addr, Instant::now()));
}

/// The phone's address if it said hello in the last minute.
pub fn phone() -> Option<std::net::SocketAddr> {
    PHONE.lock().unwrap_or_else(|e| e.into_inner()).filter(|(_, t)| t.elapsed() < Duration::from_secs(60)).map(|(a, _)| a)
}

/// Asks the phone for its newest photo. False if it hasn't said where it is.
pub fn pull(id: &[u8; 8], key: &[u8; 32]) -> bool {
    let Some(addr) = phone() else { return false };
    let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") else { return false };
    let pkt = crate::proto::pull_packet(id, key, &crate::proto::random());
    // Twice: Wi-Fi sometimes drops one.
    sock.send_to(&pkt, addr).is_ok() | sock.send_to(&pkt, addr).is_ok()
}

pub fn status() -> Status {
    use std::sync::atomic::Ordering::Relaxed;
    let last = LAST.lock().unwrap_or_else(|e| e.into_inner()).map(|(s, t)| (s, t.elapsed()));
    Status { state: STATE.load(Relaxed), last, threshold: THRESHOLD.load(Relaxed) }
}

fn set_state(s: u8) {
    STATE.store(s, std::sync::atomic::Ordering::Relaxed);
}

#[cfg_attr(not(windows), allow(dead_code))]
fn heard(smoothed: f32) {
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = Some((smoothed, Instant::now()));
}

/// Watches Bluetooth LE advertisements for this PC's phone and calls `on` for every reading.
/// Waits quietly while Bluetooth is off or missing, and restarts if Windows stops the scan.
#[cfg(windows)]
pub fn watch(key: [u8; 32], enter_dbm: i16, on: impl Fn(Reading) + Send + Sync + 'static) {
    use std::sync::{Arc, Mutex};
    use windows::Devices::Bluetooth::Advertisement::*;
    use windows::Foundation::TypedEventHandler;
    use windows::Storage::Streams::DataReader;

    let on = Arc::new(on);
    let det = Arc::new(Mutex::new(Detector::new(enter_dbm)));
    THRESHOLD.store(enter_dbm, std::sync::atomic::Ordering::Relaxed);
    set_state(NO_BLUETOOTH);
    let _ = std::thread::Builder::new().name("bluetooth".into()).spawn(move || loop {
        let started = (|| -> windows::core::Result<BluetoothLEAdvertisementWatcher> {
            let w = BluetoothLEAdvertisementWatcher::new()?;
            w.SetScanningMode(BluetoothLEScanningMode::Passive)?;
            let (on, det) = (on.clone(), det.clone());
            w.Received(&TypedEventHandler::new(
                move |_: &Option<BluetoothLEAdvertisementWatcher>, args: &Option<BluetoothLEAdvertisementReceivedEventArgs>| {
                    let Some(a) = args else { return Ok(()) };
                    let list = a.Advertisement()?.GetManufacturerDataByCompanyId(COMPANY_ID)?;
                    for i in 0..list.Size()? {
                        let buf = list.GetAt(i)?.Data()?;
                        let mut v = vec![0u8; buf.Length()? as usize];
                        DataReader::FromBuffer(&buf)?.ReadBytes(&mut v)?;
                        if matches(&key, &v, minute_now()) {
                            let rssi = a.RawSignalStrengthInDBm()?;
                            let mut d = det.lock().unwrap_or_else(|e| e.into_inner());
                            let came_near = d.feed(rssi, Instant::now());
                            let smoothed = d.smoothed().unwrap_or(rssi as f32);
                            drop(d);
                            heard(smoothed);
                            on(Reading { rssi, smoothed, came_near });
                        }
                    }
                    Ok(())
                },
            ))?;
            w.Start()?;
            Ok(w)
        })();
        match started {
            Ok(w) => {
                set_state(SCANNING);
                // Bluetooth switched off, or the adapter went to sleep: the scan stops; start over.
                while matches!(w.Status(), Ok(BluetoothLEAdvertisementWatcherStatus::Started | BluetoothLEAdvertisementWatcherStatus::Created)) {
                    std::thread::sleep(Duration::from_secs(5));
                }
                let _ = w.Stop();
                set_state(NO_BLUETOOTH);
            }
            // No adapter, or Bluetooth is off: try again soon, so switching it on is noticed.
            Err(_) => std::thread::sleep(Duration::from_secs(10)),
        }
    });
}

#[cfg(not(windows))]
pub fn watch(_key: [u8; 32], _enter_dbm: i16, _on: impl Fn(Reading) + Send + Sync + 'static) {
    set_state(NO_BLUETOOTH);
}

/// For tests: pretend the watcher heard the phone.
#[doc(hidden)]
pub fn simulate(state: u8, smoothed: Option<f32>) {
    set_state(state);
    *LAST.lock().unwrap_or_else(|e| e.into_inner()) = smoothed.map(|s| (s, Instant::now()));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_status_follows_how_recently_the_phone_was_heard() {
        assert_eq!(link_after(None), Link::Offline);
        assert_eq!(link_after(Some(Duration::from_secs(10))), Link::Connected);
        assert_eq!(link_after(Some(Duration::from_secs(90))), Link::Unstable);
        assert_eq!(link_after(Some(Duration::from_secs(600))), Link::Offline);
    }

    #[test]
    fn token_matches_only_its_key_and_nearby_minutes() {
        let key = [7u8; 32];
        let m = 29_000_000;
        let mut data = PREFIX.to_vec();
        data.extend_from_slice(&token(&key, m));
        assert!(matches(&key, &data, m));
        assert!(matches(&key, &data, m + 1)); // phone clock a minute behind
        assert!(!matches(&key, &data, m + 3));
        assert!(!matches(&[8u8; 32], &data, m));
        assert!(!matches(&key, &data[..9], m));
        assert_ne!(token(&key, m), token(&key, m + 1));
        // Shared with the Android test (ProtoTokenTest): both sides must agree byte for byte.
        assert_eq!(crate::util::hex(&token(&key, m)), "c2a8ab18665b2517");
    }

    fn run(d: &mut Detector, t0: Instant, readings: &[(u64, i16)]) -> Vec<u64> {
        readings.iter().filter(|(ms, r)| d.feed(*r, t0 + Duration::from_millis(*ms))).map(|(ms, _)| *ms).collect()
    }

    #[test]
    fn far_phone_never_triggers_and_noise_is_ignored() {
        let mut d = Detector::new(-50);
        let t0 = Instant::now();
        // Across the room, with occasional spikes.
        let r: Vec<(u64, i16)> = (0..40).map(|i| (i * 250, if i % 7 == 0 { -48 } else { -78 })).collect();
        assert!(run(&mut d, t0, &r).is_empty());
    }

    #[test]
    fn bringing_close_triggers_once_then_again_after_moving_away() {
        let mut d = Detector::new(-50);
        let t0 = Instant::now();
        let mut r: Vec<(u64, i16)> = (0..8).map(|i| (i * 250, -80)).collect(); // far
        r.extend((8..20).map(|i| (i * 250, -38))); // held next to the laptop
        let first = run(&mut d, t0, &r);
        assert_eq!(first.len(), 1);
        assert!(first[0] <= 9 * 250 + 250, "reacts within ~0.5 s, got {} ms", first[0]);

        // Stays close: no repeats. Moves away, then comes back after the cooldown: fires again.
        let mut r2: Vec<(u64, i16)> = (20..40).map(|i| (i * 250, -40)).collect();
        r2.extend((40..60).map(|i| (i * 250, -85)));
        r2.extend((100..110).map(|i| (i * 250, -36)));
        assert_eq!(run(&mut d, t0, &r2), vec![101 * 250]);
    }
}
