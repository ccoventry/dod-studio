//! Why a demo could not be written back out (#1510).
//!
//! A re-encoded frame goes back through the delta tables the parse ended on,
//! and those tables come off disk. A demo can carry one the writer cannot
//! encode with: a field described twice, a field narrower than the value it
//! holds, a width over 32 bits. Each used to be an `unwrap` or an
//! out-of-bounds slice, which the release profile's `panic = "abort"` turns
//! into the whole program closing. They are errors now, naming the table and
//! the field.

use std::fmt;

/// What stopped one delta from being encoded with its table.
#[derive(Debug, Clone, PartialEq)]
pub enum DeltaWriteError {
    /// The delta carries a field the table has no description for: the
    /// table changed after the delta was read, or the field was added by hand.
    UnknownField(String),
    /// The table describes the field more than once. The parse kept only the
    /// later value, so the original encoding cannot be rebuilt.
    DuplicateField(String),
    /// The field sits past the 56 a delta's mask can address (at most 7 mask
    /// bytes, counted in 3 bits).
    FieldOutOfMask { field: String, index: usize },
    /// The value holds fewer bytes than the field's type is written from.
    ValueTooShort {
        field: String,
        have: usize,
        need: usize,
    },
    /// The field's width cannot be written: over 32 bits, or 0 for a signed
    /// field, which has to spend one bit on the sign.
    BadWidth { field: String, bits: u32 },
    /// The field's description names no type.
    NoType(String),
}

/// What stopped one message from being written.
#[derive(Debug, Clone, PartialEq)]
pub enum WriteErrorKind {
    /// The demo holds decoded messages but no delta tables to encode them
    /// with: it was built by hand without an `Aux`.
    NoAux,
    /// A message is encoded with a delta table the demo never described.
    MissingDeltaTable(String),
    /// A delta did not fit the table it is encoded with.
    Delta {
        table: String,
        error: DeltaWriteError,
    },
}

/// A write that stopped at one frame. `frame` counts the entry's frames from
/// 0, the same index `DirectoryEntry::frames` uses.
#[derive(Debug, Clone, PartialEq)]
pub struct WriteError {
    pub entry: usize,
    pub frame: usize,
    pub time: f32,
    pub kind: WriteErrorKind,
}

/// A field or table name without the NUL it carries off the wire.
fn shown(name: &str) -> &str {
    name.trim_end_matches('\0')
}

impl fmt::Display for DeltaWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownField(field) => {
                write!(f, "field `{}` is not in the table", shown(field))
            }
            Self::DuplicateField(field) => {
                write!(f, "field `{}` is described more than once", shown(field))
            }
            Self::FieldOutOfMask { field, index } => write!(
                f,
                "field `{}` is number {index}, past the 56 a delta can carry",
                shown(field)
            ),
            Self::ValueTooShort { field, have, need } => write!(
                f,
                "field `{}` holds {have} bytes where its type needs {need}",
                shown(field)
            ),
            Self::BadWidth { field, bits } => {
                write!(f, "field `{}` is {bits} bits wide", shown(field))
            }
            Self::NoType(field) => write!(f, "field `{}` has no type", shown(field)),
        }
    }
}

impl fmt::Display for WriteErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoAux => write!(
                f,
                "the demo has no delta tables to encode its messages with"
            ),
            Self::MissingDeltaTable(table) => {
                write!(f, "the demo has no `{}` delta table", shown(table))
            }
            Self::Delta { table, error } => {
                write!(f, "delta table `{}`: {error}", shown(table))
            }
        }
    }
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "could not re-encode frame {} of directory entry {} ({:.2} s): {}",
            self.frame, self.entry, self.time, self.kind
        )
    }
}

impl std::error::Error for WriteError {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_demo_from_bytes;
    use crate::types::{Demo, EngineMessage, FrameData, MessageData, NetMessage};

    fn fixture() -> Vec<u8> {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../test-fixtures/ci_fixture.dem"
        );
        std::fs::read(path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    /// `ci_fixture.dem` with a second `entity_state_t` description in its last
    /// network frame, carrying `weapon_data_t`'s fields: a demo whose table for
    /// one struct changes mid-file. It parses, since nothing after it is read
    /// with the new table, but the writer re-encodes every earlier entity delta
    /// with the table the parse ended on, which lacks their fields.
    fn table_replaced_at_the_end() -> Vec<u8> {
        let mut demo = open_demo_from_bytes(&fixture()).unwrap();
        let weapon_data = demo
            .directory
            .entries
            .iter()
            .flat_map(|e| &e.frames)
            .find_map(|frame| match &frame.frame_data {
                FrameData::NetworkMessage(network) => match &network.1.messages {
                    MessageData::Parsed(messages) => messages.iter().find_map(|m| match m {
                        NetMessage::EngineMessage(e) => match &**e {
                            EngineMessage::SvcDeltaDescription(d)
                                if d.name == b"weapon_data_t\0" =>
                            {
                                Some(d.clone())
                            }
                            _ => None,
                        },
                        _ => None,
                    }),
                    _ => None,
                },
                _ => None,
            })
            .expect("the fixture describes weapon_data_t");
        let mut entity_state = weapon_data;
        entity_state.name = b"entity_state_t\0".to_vec();

        let last = demo
            .directory
            .entries
            .iter_mut()
            .flat_map(|e| e.frames.iter_mut())
            .filter_map(|frame| match &mut frame.frame_data {
                FrameData::NetworkMessage(network) => Some(&mut network.1),
                _ => None,
            })
            .last()
            .expect("the fixture has network frames");
        if let MessageData::Parsed(messages) = last.messages_mut() {
            messages.push(NetMessage::EngineMessage(Box::new(
                EngineMessage::SvcDeltaDescription(entity_state),
            )));
        }
        demo.write_to_bytes().unwrap()
    }

    fn parsed(bytes: &[u8]) -> Demo {
        open_demo_from_bytes(bytes).expect("a table changed mid-file still parses")
    }

    #[test]
    fn a_table_changed_mid_file_fails_the_write_naming_the_table() {
        let demo = parsed(&table_replaced_at_the_end());
        let error = demo
            .write_to_bytes()
            .expect_err("entity deltas lost their fields");
        assert!(
            matches!(
                &error.kind,
                WriteErrorKind::Delta {
                    table,
                    error: DeltaWriteError::UnknownField(_)
                } if table == "entity_state_t\0"
            ),
            "{error}"
        );
        assert!(error.to_string().contains("`entity_state_t`"), "{error}");
    }

    /// The copying writer re-encodes only edited frames, so an untouched
    /// demo is still written whatever its tables; an edited frame is not.
    #[test]
    fn the_copying_writer_fails_only_on_a_frame_it_re_encodes() {
        let bytes = table_replaced_at_the_end();
        let mut demo = parsed(&bytes);
        let untouched = demo
            .write_to_bytes_reusing_source_cancellable(&bytes, &|| false)
            .expect("nothing is re-encoded")
            .expect("not cancelled");
        assert!(untouched == bytes);

        for frame in demo
            .directory
            .entries
            .iter_mut()
            .flat_map(|e| &mut e.frames)
        {
            if let FrameData::NetworkMessage(network) = &mut frame.frame_data {
                network.1.messages_mut();
            }
        }
        assert!(
            demo.write_to_bytes_reusing_source_cancellable(&bytes, &|| false)
                .is_err()
        );
    }

    /// The file is encoded before it is opened, so a failed write leaves
    /// what was there.
    #[test]
    fn a_failed_write_to_file_leaves_the_file_alone() {
        let demo = parsed(&table_replaced_at_the_end());
        let path = std::env::temp_dir().join(format!("dem_write_error_{}.dem", std::process::id()));
        std::fs::write(&path, b"kept").unwrap();
        let result = demo.write_to_file(&path);
        let kept = std::fs::read(&path);
        let _ = std::fs::remove_file(&path);
        assert!(result.is_err());
        assert_eq!(kept.unwrap(), b"kept");
    }

    #[test]
    fn decoded_messages_without_delta_tables_are_an_error() {
        let mut demo = parsed(&fixture());
        demo._aux = None;
        assert_eq!(
            demo.write_to_bytes().map(|_| ()).map_err(|e| e.kind),
            Err(WriteErrorKind::NoAux)
        );
    }
}
