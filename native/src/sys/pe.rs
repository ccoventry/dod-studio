//! Reading a Windows executable's PE header without loading it.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// `IMAGE_FILE_LARGE_ADDRESS_AWARE`: a 32-bit process marked with it gets
/// 4 GB of address space on 64-bit Windows instead of 2 GB (#430). The 25th
/// Anniversary `hl.exe` has it; the pre-Anniversary one doesn't, which is
/// what big HD textures run out of.
const IMAGE_FILE_LARGE_ADDRESS_AWARE: u16 = 0x20;

/// Whether the executable at `path` is large-address-aware. `Err` when the
/// file can't be read or isn't a PE executable.
pub fn is_large_address_aware(path: &Path) -> std::io::Result<bool> {
    let mut file = std::fs::File::open(path)?;
    characteristics(&mut file).map(|c| c & IMAGE_FILE_LARGE_ADDRESS_AWARE != 0)
}

/// The COFF file header's `Characteristics` field.
fn characteristics(reader: &mut (impl Read + Seek)) -> std::io::Result<u16> {
    let bad = |what: &str| std::io::Error::new(std::io::ErrorKind::InvalidData, what.to_string());
    let mut dos = [0u8; 64];
    reader.read_exact(&mut dos)?;
    if &dos[..2] != b"MZ" {
        return Err(bad("not an MZ executable"));
    }
    let pe_offset = u32::from_le_bytes([dos[60], dos[61], dos[62], dos[63]]) as u64;
    reader.seek(SeekFrom::Start(pe_offset))?;
    // "PE\0\0", then Machine, NumberOfSections, TimeDateStamp,
    // PointerToSymbolTable, NumberOfSymbols, SizeOfOptionalHeader,
    // Characteristics.
    let mut header = [0u8; 24];
    reader.read_exact(&mut header)?;
    if &header[..4] != b"PE\0\0" {
        return Err(bad("no PE signature"));
    }
    Ok(u16::from_le_bytes([header[22], header[23]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_pe(characteristics: u16) -> std::io::Cursor<Vec<u8>> {
        let mut bytes = vec![0u8; 0x80 + 24];
        bytes[..2].copy_from_slice(b"MZ");
        bytes[60..64].copy_from_slice(&0x80u32.to_le_bytes());
        bytes[0x80..0x84].copy_from_slice(b"PE\0\0");
        bytes[0x80 + 22..0x80 + 24].copy_from_slice(&characteristics.to_le_bytes());
        std::io::Cursor::new(bytes)
    }

    #[test]
    fn reads_the_flag_and_refuses_non_executables() {
        assert_eq!(
            characteristics(&mut fake_pe(0x0122)).unwrap() & IMAGE_FILE_LARGE_ADDRESS_AWARE,
            0x20
        );
        assert_eq!(
            characteristics(&mut fake_pe(0x0102)).unwrap() & IMAGE_FILE_LARGE_ADDRESS_AWARE,
            0
        );
        assert!(characteristics(&mut std::io::Cursor::new(vec![0u8; 200])).is_err());
    }

    /// The real installs, when they're on this machine: the pre-Anniversary
    /// `hl.exe` is 2 GB, the 25th Anniversary one 4 GB (#430). Skipped where
    /// they aren't installed.
    #[test]
    fn the_real_installs_read_as_expected() {
        let steam = Path::new(r"C:\Program Files (x86)\Steam\steamapps\common");
        for (install, expected) in [
            ("Half-Life - PRE-Anniversary for Movies", false),
            ("Half-Life", true),
        ] {
            let exe = steam.join(install).join("hl.exe");
            if exe.is_file() {
                assert_eq!(
                    is_large_address_aware(&exe).unwrap(),
                    expected,
                    "{}",
                    exe.display()
                );
            }
        }
    }
}
