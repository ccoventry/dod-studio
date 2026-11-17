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
            Self::NoAux => write!(f, "the demo has no delta tables to encode its messages with"),
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
