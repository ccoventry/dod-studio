//! One `call rel32` in `hw.dll` that a setting rewrites while it is on and
//! puts back while it is off (#677): `engine_center_print` turns it into
//! `nop`s, `plugin_text` points it at a filter of ours.
//!
//! Each engine build gets its own signature ([`Build`]), and each signature
//! pins the call's own `rel32`. A `rel32` between two functions of one module
//! is not relocated, so a match proves where the call goes as well as where
//! it is, and a build whose call goes somewhere else simply does not match.
//!
//! `hw.dll` never reloads, so the scan runs once, the first time the setting
//! is on. After that every frame is a 5-byte compare. The stock bytes are
//! read at the scan, so turning the setting off writes back exactly what was
//! there; a span holding anything but those or our own bytes means someone
//! else has patched it, and it is left alone.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use crate::engine;
use crate::scan;

/// `E8` and a 32-bit displacement.
pub(crate) const CALL_LEN: usize = 5;

/// One engine build's signature for the call.
pub(crate) struct Build {
    pub name: &'static str,
    /// Unique in that build's `hw.dll`, with the call's `rel32` pinned.
    pub pattern: &'static str,
    /// Where the call's `E8` is in `pattern`.
    pub at: usize,
}

/// The call, once found.
pub(crate) struct CallSite {
    what: &'static str,
    builds: &'static [Build],
    /// The call's address, 0 before a successful scan.
    address: AtomicUsize,
    /// The stock 5 bytes, little-endian.
    stock: AtomicU64,
    /// Index into `builds` plus one, 0 before a successful scan.
    build: AtomicUsize,
    /// Set when no build matched, so the scan is not repeated every frame.
    failed: AtomicBool,
}

/// The bytes of `call to` placed at `from`.
pub(crate) fn call_bytes(from: usize, to: usize) -> [u8; CALL_LEN] {
    let rel = (to as u32).wrapping_sub((from as u32).wrapping_add(CALL_LEN as u32));
    let mut out = [0xe8, 0, 0, 0, 0];
    out[1..].copy_from_slice(&rel.to_le_bytes());
    out
}

/// Where the `call rel32` in `bytes`, placed at `from`, goes.
pub(crate) fn call_target(from: usize, bytes: &[u8; CALL_LEN]) -> usize {
    let rel = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]);
    (from as u32)
        .wrapping_add(CALL_LEN as u32)
        .wrapping_add(rel) as usize
}

impl CallSite {
    pub(crate) const fn new(what: &'static str, builds: &'static [Build]) -> Self {
        Self {
            what,
            builds,
            address: AtomicUsize::new(0),
            stock: AtomicU64::new(0),
            build: AtomicUsize::new(0),
            failed: AtomicBool::new(false),
        }
    }

    /// The call's address, scanning for it the first time.
    pub(crate) fn resolve(&self) -> Result<usize, String> {
        let cached = self.address.load(Ordering::Acquire);
        if cached != 0 {
            return Ok(cached);
        }
        if self.failed.load(Ordering::Acquire) {
            return Err(format!("{} was not found in this hw.dll", self.what));
        }
        let base = engine::engine_module_base().ok_or("hw.dll is not loaded yet")?;
        let mut misses = Vec::new();
        for (index, build) in self.builds.iter().enumerate() {
            // Safety: `engine_module_base` only returns a base for a mapped
            // module, and hw.dll stays mapped for the session.
            let found = match unsafe { scan::find_unique(base, build.pattern) } {
                Ok(found) => found,
                Err(why) => {
                    misses.push(format!("{} signature: {why}", build.name));
                    continue;
                }
            };
            let address = found + build.at;
            // Safety: the pattern matched, and it reaches past the call.
            let mut stock = [0u8; CALL_LEN];
            stock.copy_from_slice(unsafe {
                std::slice::from_raw_parts(address as *const u8, CALL_LEN)
            });
            if stock[0] != 0xe8 {
                self.failed.store(true, Ordering::Release);
                return Err(format!(
                    "{} in the {} hw.dll is {stock:02x?}, not a call",
                    self.what, build.name
                ));
            }
            let mut packed = [0u8; 8];
            packed[..CALL_LEN].copy_from_slice(&stock);
            self.stock
                .store(u64::from_le_bytes(packed), Ordering::Release);
            self.build.store(index + 1, Ordering::Release);
            self.address.store(address, Ordering::Release);
            return Ok(address);
        }
        self.failed.store(true, Ordering::Release);
        Err(format!(
            "could not find {} -- {}",
            self.what,
            misses.join("; ")
        ))
    }

    /// The stock bytes, once found.
    fn stock(&self) -> [u8; CALL_LEN] {
        let packed = self.stock.load(Ordering::Acquire).to_le_bytes();
        let mut out = [0u8; CALL_LEN];
        out.copy_from_slice(&packed[..CALL_LEN]);
        out
    }

    /// The function the stock call goes to, once found.
    pub(crate) fn original_target(&self) -> Option<usize> {
        let address = self.address.load(Ordering::Acquire);
        (address != 0).then(|| call_target(address, &self.stock()))
    }

    /// The build the call was found in, once found.
    pub(crate) fn build_name(&self) -> Option<&'static str> {
        let index = self.build.load(Ordering::Acquire);
        index
            .checked_sub(1)
            .and_then(|i| self.builds.get(i))
            .map(|b| b.name)
    }

    /// Writes `ours` while `on`, the stock bytes otherwise. Returns whether
    /// anything was written. `ours` must be what this module writes when on,
    /// since it is also what a revert expects to find.
    pub(crate) fn put(&self, on: bool, ours: [u8; CALL_LEN]) -> Result<bool, String> {
        let address = self.resolve()?;
        let stock = self.stock();
        let want = if on { ours } else { stock };
        // Safety: `resolve` proved the span is mapped code.
        let present = unsafe { std::slice::from_raw_parts(address as *const u8, CALL_LEN) };
        if present == want {
            return Ok(false);
        }
        if present != stock && present != ours {
            return Err(format!(
                "{} holds {present:02x?}, neither the stock {stock:02x?} nor {ours:02x?} -- something else has patched it",
                self.what
            ));
        }
        // Safety: as above. Written from the game's own thread, between
        // frames, so the engine is never inside the five bytes.
        if !unsafe { crate::patch::write_code_bytes(address, &want) } {
            return Err(format!("could not make {} writable", self.what));
        }
        Ok(true)
    }
}

/// Checks a [`Build`] table the way every user of this module needs: hex or
/// wildcard tokens, a pinned `E8 rel32` at `at`, and something after it.
#[cfg(test)]
pub(crate) fn check_builds(builds: &[Build]) {
    for build in builds {
        let tokens: Vec<&str> = build.pattern.split_whitespace().collect();
        assert!(
            crate::scan::Pattern::parse(build.pattern).is_ok(),
            "{}",
            build.name
        );
        assert!(tokens.len() > build.at + CALL_LEN, "{}", build.name);
        assert_eq!(tokens[build.at], "E8", "{}: a call rel32", build.name);
        assert!(
            tokens[build.at + 1..build.at + CALL_LEN]
                .iter()
                .all(|t| *t != "??"),
            "{}: the call's rel32 is pinned",
            build.name
        );
    }
    assert_ne!(builds[0].pattern, builds[1].pattern);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_call_goes_where_its_bytes_say() {
        // The pre-Anniversary TE_TEXTMESSAGE call: hw+0x23da4 -> hw+0x19ab0.
        let bytes = [0xe8, 0x07, 0x5d, 0xff, 0xff];
        assert_eq!(call_target(0x1d2_3da4, &bytes), 0x1d1_9ab0);
        assert_eq!(call_bytes(0x1d2_3da4, 0x1d1_9ab0), bytes);
    }

    #[test]
    fn a_call_can_reach_anywhere_in_the_address_space() {
        for (from, to) in [
            (0x1000_0000, 0x7fff_0000),
            (0x7fff_0000, 0x1000_0000),
            (0x0100_0000, 0xf000_0000),
        ] {
            assert_eq!(call_target(from, &call_bytes(from, to)), to);
        }
    }
}
