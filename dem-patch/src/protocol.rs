//! The network protocols the parsers read (#239).
//!
//! A demo's header names the network protocol it was recorded with, and so
//! does every `svc_serverinfo` in it. The parsers were written for protocol
//! 48, every DoD 1.3 demo recorded since 2004. Protocol 47, the one before
//! it, lays its frames, messages and delta encoding out the same way: 77 of
//! 78 protocol-47 match demos from 2004-2005 parse and analyse with no change,
//! the 78th is a copy cut short (`docs/goldsrc_protocol_47.md`). Only the game
//! tells them apart, and refuses to play a protocol-47 demo until it is
//! converted (`native::patch::protocol47`).
//!
//! No other protocol has been checked against a real demo, so the header
//! parse refuses one by name rather than read it with rules that may not hold.

use std::fmt;

/// The protocol of every Steam-era DoD 1.3 demo since 2004, and the only one
/// today's game plays.
pub const NETWORK_PROTOCOL_48: i32 = 48;
/// The protocol before it: parses as 48 does, plays once converted.
pub const NETWORK_PROTOCOL_47: i32 = 47;

/// A header's network protocol the parsers don't read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnsupportedProtocol {
    pub network_protocol: i32,
}

impl fmt::Display for UnsupportedProtocol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "network protocol {} is not supported: only demos of protocol {} and {} can be read",
            self.network_protocol, NETWORK_PROTOCOL_47, NETWORK_PROTOCOL_48
        )
    }
}

impl std::error::Error for UnsupportedProtocol {}

/// `Ok` for a network protocol the parsers read (47 or 48), else the error
/// that names it.
pub fn check_network_protocol(network_protocol: i32) -> Result<(), UnsupportedProtocol> {
    match network_protocol {
        NETWORK_PROTOCOL_47 | NETWORK_PROTOCOL_48 => Ok(()),
        _ => Err(UnsupportedProtocol { network_protocol }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocols_47_and_48_are_read() {
        assert_eq!(check_network_protocol(47), Ok(()));
        assert_eq!(check_network_protocol(48), Ok(()));
    }

    #[test]
    fn any_other_protocol_is_refused_by_number() {
        for protocol in [0, 46, 49, -1] {
            let err = check_network_protocol(protocol).unwrap_err();
            assert_eq!(err.network_protocol, protocol);
            assert!(
                err.to_string().contains(&format!("protocol {protocol} ")),
                "{err}"
            );
        }
    }

    /// A bare 544-byte header: magic, demo protocol 5, `network_protocol`,
    /// map and folder names, no directory.
    fn header(network_protocol: i32) -> Vec<u8> {
        let mut h = vec![0u8; 544];
        h[..8].copy_from_slice(b"HLDEMO\0\0");
        h[8..12].copy_from_slice(&5i32.to_le_bytes());
        h[12..16].copy_from_slice(&network_protocol.to_le_bytes());
        h[16..26].copy_from_slice(b"dod_donner");
        h[276..279].copy_from_slice(b"dod");
        h
    }

    #[test]
    fn a_protocol_47_header_parses_as_48_s_does() {
        for protocol in [47, 48] {
            let bytes = header(protocol);
            let (rest, parsed) = crate::demo_parser::parse_header(&bytes).unwrap();
            assert!(rest.is_empty());
            assert_eq!(parsed.network_protocol, protocol);
            assert_eq!(parsed.demo_protocol, 5);
        }
    }

    #[test]
    fn a_demo_of_another_protocol_is_refused_in_words() {
        assert!(crate::demo_parser::parse_header(&header(46)).is_err());
        let reason = crate::types::Demo::parse_from_bytes_tolerant(
            &header(46),
            crate::types::MessageDataParseMode::Parse,
        )
        .unwrap_err()
        .to_string();
        assert!(
            reason.contains("network protocol 46 is not supported"),
            "{reason}"
        );
    }
}
