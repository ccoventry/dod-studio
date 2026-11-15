//! Asks a server what it is before `connect_guard` lets the game join it.
//!
//! A GoldSrc server or HLTV proxy answers the connectionless `A2S_INFO`
//! query (`FF FF FF FF` `TSource Engine Query\0`) with its details: an `I`
//! reply (the Source-era layout), an `m` reply (GoldSrc's older one), or
//! both. Each carries a server-type byte -- `d` dedicated, `l` listen, `p` an
//! HLTV proxy -- and a VAC byte. A server may first answer `A` with a
//! four-byte challenge, which the query is then resent with.
//!
//! Read from both builds of `proxy.dll` (pre-Anniversary RVAs): the proxy
//! checks only the query's first byte, `T` (`+0x13500`), and answers with an
//! `m` reply only, type `p` (`+0x15d4c`) and a VAC byte hard-coded to 0
//! (`+0x15df3`), with no challenge and no bots byte. A proxy that isn't
//! relaying a game answers nothing. Steam's game-server library sees each
//! packet first and may send an `I` reply of its own, so a proxy can answer
//! twice and the two needn't agree on the type: see [`combine`].
//!
//! This only parses and asks; `connect_guard` decides what the answer allows.

use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

/// The port `connect <host>` uses when the address names none.
pub const DEFAULT_PORT: u16 = 27015;

const QUERY: &[u8] = b"\xff\xff\xff\xffTSource Engine Query\0";

/// What a server said about itself.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ServerInfo {
    /// The server-type byte, lowercased: `b'p'` for an HLTV proxy.
    pub server_type: u8,
    /// Whether it says VAC is on.
    pub vac: bool,
}

impl ServerInfo {
    pub fn is_hltv(&self) -> bool {
        self.server_type == b'p'
    }
}

/// Why there is no [`ServerInfo`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum QueryError {
    /// The address didn't resolve.
    BadAddress(String),
    /// Nothing answered in time.
    NoAnswer,
    /// Something answered, but not with a reply this can read.
    Unreadable,
}

/// One reply packet, read.
#[derive(Debug, PartialEq, Eq)]
enum Reply {
    Info(ServerInfo),
    Challenge([u8; 4]),
}

/// Reads one reply packet: `FF FF FF FF` then `I`, `m` or `A`.
fn parse_reply(packet: &[u8]) -> Option<Reply> {
    let body = packet.strip_prefix(b"\xff\xff\xff\xff")?;
    let (&kind, body) = body.split_first()?;
    match kind {
        b'A' => Some(Reply::Challenge(body.get(..4)?.try_into().ok()?)),
        b'I' => parse_source_info(body).map(Reply::Info),
        b'm' => parse_goldsrc_info(body).map(Reply::Info),
        _ => None,
    }
}

/// Skips one NUL-terminated string.
fn skip_cstr(body: &[u8]) -> Option<&[u8]> {
    let end = body.iter().position(|&b| b == 0)?;
    Some(&body[end + 1..])
}

/// `I`: protocol, name, map, folder, game, app id (2), players, max players,
/// bots, server type, environment, visibility, VAC.
fn parse_source_info(body: &[u8]) -> Option<ServerInfo> {
    let mut rest = body.get(1..)?;
    for _ in 0..4 {
        rest = skip_cstr(rest)?;
    }
    let fields = rest.get(2..9)?;
    Some(ServerInfo {
        server_type: fields[3].to_ascii_lowercase(),
        vac: fields[6] != 0,
    })
}

/// `m`: address, name, map, folder, game, players, max players, protocol,
/// server type, environment, visibility, mod (and, when mod is 1, its info
/// link, download link and a third string, version (4), size (4), type,
/// DLL), then VAC.
fn parse_goldsrc_info(body: &[u8]) -> Option<ServerInfo> {
    let mut rest = body;
    for _ in 0..5 {
        rest = skip_cstr(rest)?;
    }
    let fields = rest.get(..7)?;
    let server_type = fields[3].to_ascii_lowercase();
    let mut rest = &rest[7..];
    if fields[6] == 1 {
        for _ in 0..3 {
            rest = skip_cstr(rest)?;
        }
        rest = rest.get(4 + 4 + 1 + 1..)?;
    }
    Some(ServerInfo {
        server_type,
        vac: *rest.first()? != 0,
    })
}

/// Combines every reply a server sent: a proxy if any reply says so (only
/// `proxy.dll`'s own `m` reply does; Steam's may call it something else), VAC
/// on if any says so.
fn combine(replies: &[ServerInfo]) -> Option<ServerInfo> {
    let first = *replies.first()?;
    Some(ServerInfo {
        server_type: if replies.iter().any(ServerInfo::is_hltv) {
            b'p'
        } else {
            first.server_type
        },
        vac: replies.iter().any(|r| r.vac),
    })
}

/// `host[:port]` as `connect` takes it, resolved.
pub fn resolve(address: &str) -> Result<SocketAddr, QueryError> {
    let address = address.trim();
    let with_port = if address.contains(':') {
        address.to_string()
    } else {
        format!("{address}:{DEFAULT_PORT}")
    };
    with_port
        .to_socket_addrs()
        .ok()
        .and_then(|mut found| found.find(SocketAddr::is_ipv4))
        .ok_or_else(|| QueryError::BadAddress(address.to_string()))
}

/// Asks `address` what it is, waiting up to `timeout` in all. Blocks: call it
/// off the game thread.
pub fn query(address: &str, timeout: Duration) -> Result<ServerInfo, QueryError> {
    let target = resolve(address)?;
    let socket = UdpSocket::bind("0.0.0.0:0").map_err(|_| QueryError::NoAnswer)?;
    socket
        .send_to(QUERY, target)
        .map_err(|_| QueryError::NoAnswer)?;

    let deadline = Instant::now() + timeout;
    let mut replies = Vec::new();
    let mut unreadable = false;
    let mut buffer = [0u8; 1400];
    // After the first details reply, a moment longer for a second one (a
    // server can send both layouts).
    let mut settle_by = None;
    loop {
        let now = Instant::now();
        let until = settle_by.unwrap_or(deadline).min(deadline);
        if now >= until {
            break;
        }
        let _ = socket.set_read_timeout(Some(until - now));
        let Ok((len, from)) = socket.recv_from(&mut buffer) else {
            break;
        };
        if from != target {
            continue;
        }
        match parse_reply(&buffer[..len]) {
            Some(Reply::Info(info)) => {
                replies.push(info);
                settle_by.get_or_insert(Instant::now() + Duration::from_millis(150));
            }
            Some(Reply::Challenge(challenge)) => {
                let mut again = QUERY.to_vec();
                again.extend_from_slice(&challenge);
                let _ = socket.send_to(&again, target);
            }
            None => unreadable = true,
        }
    }
    combine(&replies).ok_or(if unreadable {
        QueryError::Unreadable
    } else {
        QueryError::NoAnswer
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source_info(server_type: u8, vac: u8) -> Vec<u8> {
        let mut p = b"\xff\xff\xff\xffI\x30".to_vec();
        for s in ["Name", "dod_anzio", "dod", "Day of Defeat"] {
            p.extend_from_slice(s.as_bytes());
            p.push(0);
        }
        p.extend_from_slice(&30u16.to_le_bytes());
        p.extend_from_slice(&[4, 16, 0, server_type, b'w', 0, vac]);
        p
    }

    fn goldsrc_info(server_type: u8, mod_flag: u8, vac: u8) -> Vec<u8> {
        let mut p = b"\xff\xff\xff\xffm".to_vec();
        for s in ["1.2.3.4:27020", "Name", "dod_anzio", "dod", "Day of Defeat"] {
            p.extend_from_slice(s.as_bytes());
            p.push(0);
        }
        p.extend_from_slice(&[4, 16, 47, server_type, b'w', 0, mod_flag]);
        if mod_flag == 1 {
            p.extend_from_slice(b"http://a\0http://b\0third\0");
            p.extend_from_slice(&[1, 0, 0, 0, 2, 0, 0, 0, 0, 1]);
        }
        p.extend_from_slice(&[vac, 0]);
        p
    }

    #[test]
    fn reads_an_hltv_proxy_from_either_layout() {
        for packet in [
            source_info(b'p', 0),
            goldsrc_info(b'p', 0, 0),
            goldsrc_info(b'P', 1, 0),
        ] {
            let Some(Reply::Info(info)) = parse_reply(&packet) else {
                panic!("{packet:?}");
            };
            assert!(info.is_hltv());
            assert!(!info.vac);
        }
    }

    #[test]
    fn reads_the_reply_proxy_dll_builds() {
        // As `proxy.dll` writes it: protocol 48, `p`, `w`, no password, not a
        // mod, VAC 0, and nothing after (no bots byte).
        let mut packet = b"\xff\xff\xff\xffm".to_vec();
        for s in ["1.2.3.4:27020", "HLTV", "dod_anzio", "dod", "Day of Defeat"] {
            packet.extend_from_slice(s.as_bytes());
            packet.push(0);
        }
        packet.extend_from_slice(&[3, 100, 48, b'p', b'w', 0, 0, 0]);
        assert_eq!(
            parse_reply(&packet),
            Some(Reply::Info(ServerInfo {
                server_type: b'p',
                vac: false
            }))
        );
    }

    #[test]
    fn reads_a_dedicated_server_and_its_vac_byte() {
        let Some(Reply::Info(info)) = parse_reply(&source_info(b'd', 1)) else {
            panic!();
        };
        assert!(!info.is_hltv());
        assert!(info.vac);
        let Some(Reply::Info(info)) = parse_reply(&goldsrc_info(b'D', 1, 1)) else {
            panic!();
        };
        assert!(!info.is_hltv());
        assert!(info.vac);
    }

    #[test]
    fn reads_a_challenge() {
        assert_eq!(
            parse_reply(b"\xff\xff\xff\xffA\x01\x02\x03\x04"),
            Some(Reply::Challenge([1, 2, 3, 4]))
        );
    }

    #[test]
    fn a_short_or_foreign_packet_is_not_a_reply() {
        assert_eq!(parse_reply(b"\xff\xff\xff\xffI\x30Name"), None);
        assert_eq!(parse_reply(b"\xff\xff\xff\xffA\x01"), None);
        assert_eq!(parse_reply(b"\xfe\xff\xff\xffI"), None);
        assert_eq!(parse_reply(b"\xff\xff\xff\xffj"), None);
        let mut cut = goldsrc_info(b'p', 0, 0);
        cut.truncate(cut.len() - 2);
        assert_eq!(parse_reply(&cut), None);
    }

    #[test]
    fn a_proxy_if_any_reply_says_so_vac_on_if_any_says_so() {
        let proxy = ServerInfo {
            server_type: b'p',
            vac: false,
        };
        let server = ServerInfo {
            server_type: b'd',
            vac: false,
        };
        let secure = ServerInfo {
            server_type: b'p',
            vac: true,
        };
        assert_eq!(combine(&[proxy, proxy]), Some(proxy));
        // Steam's own reply beside proxy.dll's.
        assert_eq!(combine(&[server, proxy]), Some(proxy));
        assert!(!combine(&[server, server]).unwrap().is_hltv());
        assert!(combine(&[proxy, secure]).unwrap().vac);
        assert_eq!(combine(&[]), None);
    }

    #[test]
    fn an_address_without_a_port_uses_27015() {
        assert_eq!(resolve("127.0.0.1").unwrap().port(), DEFAULT_PORT);
        assert_eq!(resolve("127.0.0.1:27020").unwrap().port(), 27020);
        assert!(matches!(
            resolve("not an address"),
            Err(QueryError::BadAddress(_))
        ));
    }

    #[test]
    fn a_silent_server_gives_no_answer() {
        // A socket that is bound and kept open but never replies. Not a port
        // bound and dropped: the OS can hand that port straight to another
        // test running in parallel, whose server then answers (seen in CI,
        // 2026-10-06: Unreadable instead of NoAnswer).
        let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = silent.local_addr().unwrap().port();
        let result = query(&format!("127.0.0.1:{port}"), Duration::from_millis(300));
        drop(silent);
        assert_eq!(result, Err(QueryError::NoAnswer));
    }

    #[test]
    fn asks_and_follows_a_challenge() {
        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        let address = server.local_addr().unwrap().to_string();
        let thread = std::thread::spawn(move || {
            let mut buf = [0u8; 256];
            let (len, from) = server.recv_from(&mut buf).unwrap();
            assert_eq!(&buf[..len], QUERY);
            server
                .send_to(b"\xff\xff\xff\xffA\x09\x08\x07\x06", from)
                .unwrap();
            let (len, from) = server.recv_from(&mut buf).unwrap();
            assert_eq!(&buf[QUERY.len()..len], &[9, 8, 7, 6]);
            server.send_to(&source_info(b'p', 0), from).unwrap();
        });
        let info = query(&address, Duration::from_secs(2)).unwrap();
        thread.join().unwrap();
        assert!(info.is_hltv());
        assert!(!info.vac);
    }
}
