//! How a decoded delta is stored: the fields one delta update set, each a name
//! and its little-endian value bytes.
//!
//! It was a `HashMap<String, Vec<u8>>`: one hash table, one key string and one
//! value vector per field, which made the delta tables over half the heap
//! blocks a demo's decode allocates (#37). Here a delta is one vector of
//! fields, the names are shared with the decoder that read them (`Arc<str>`,
//! no copy), and a value of up to four bytes, every number type a delta
//! carries, lives inline. Strings longer than that still get a block of their
//! own. Nothing is dropped: the same names and bytes come back out, now in the
//! order the decoder lists the fields rather than in hash order.

use std::fmt;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

/// A delta field's name as the decoder spells it, NUL included for names that
/// came off the wire (`"origin[0]\0"`).
pub type FieldName = Arc<str>;

/// The fields one delta update set. Lookups are a scan, which beats hashing
/// for the handful of fields an update carries and stays fast for a full
/// entity state (~40 fields).
#[derive(Clone, Default, PartialEq)]
pub struct Delta {
    fields: Vec<(FieldName, DeltaValue)>,
}

impl Delta {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            fields: Vec::with_capacity(capacity),
        }
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn get(&self, key: &str) -> Option<&DeltaValue> {
        self.position(key).map(|at| &self.fields[at].1)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.position(key).is_some()
    }

    /// Sets `key` to `value`, returning the value it replaces.
    pub fn insert(
        &mut self,
        key: impl Into<FieldName>,
        value: impl Into<DeltaValue>,
    ) -> Option<DeltaValue> {
        let key = key.into();
        let value = value.into();
        match self.position_of(&key) {
            Some(at) => Some(std::mem::replace(&mut self.fields[at].1, value)),
            None => {
                self.fields.push((key, value));
                None
            }
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<DeltaValue> {
        self.position(key).map(|at| self.fields.remove(at).1)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&FieldName, &DeltaValue)> {
        self.fields.iter().map(|(k, v)| (k, v))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&FieldName, &mut DeltaValue)> {
        self.fields.iter_mut().map(|(k, v)| (&*k, v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &FieldName> {
        self.fields.iter().map(|(k, _)| k)
    }

    pub fn values(&self) -> impl Iterator<Item = &DeltaValue> {
        self.fields.iter().map(|(_, v)| v)
    }

    fn position(&self, key: &str) -> Option<usize> {
        self.fields.iter().position(|(k, _)| &**k == key)
    }

    /// Like `position`, but tries the shared name first: a field applied over
    /// an entity's state almost always carries the same `Arc` as the field it
    /// replaces, since both came from one decoder.
    fn position_of(&self, key: &FieldName) -> Option<usize> {
        self.fields
            .iter()
            .position(|(k, _)| Arc::ptr_eq(k, key))
            .or_else(|| self.position(key))
    }
}

impl fmt::Debug for Delta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<K: Into<FieldName>, V: Into<DeltaValue>> Extend<(K, V)> for Delta {
    fn extend<I: IntoIterator<Item = (K, V)>>(&mut self, iter: I) {
        for (k, v) in iter {
            self.insert(k, v);
        }
    }
}

impl<K: Into<FieldName>, V: Into<DeltaValue>> FromIterator<(K, V)> for Delta {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut delta = Delta::new();
        delta.extend(iter);
        delta
    }
}

impl IntoIterator for Delta {
    type Item = (FieldName, DeltaValue);
    type IntoIter = std::vec::IntoIter<(FieldName, DeltaValue)>;

    fn into_iter(self) -> Self::IntoIter {
        self.fields.into_iter()
    }
}

impl<'a> IntoIterator for &'a Delta {
    type Item = (&'a FieldName, &'a DeltaValue);
    type IntoIter = std::iter::Map<
        std::slice::Iter<'a, (FieldName, DeltaValue)>,
        fn(&'a (FieldName, DeltaValue)) -> (&'a FieldName, &'a DeltaValue),
    >;

    fn into_iter(self) -> Self::IntoIter {
        self.fields.iter().map(|(k, v)| (k, v))
    }
}

/// Bytes held inline. Four covers every number a delta field decodes to
/// (`delta.rs`: byte, short, integer, float, angle).
const INLINE: usize = 4;

/// One field's value, little-endian as the decoder wrote it. Reads as a
/// `[u8]`.
#[derive(Clone)]
pub enum DeltaValue {
    Inline { len: u8, bytes: [u8; INLINE] },
    Heap(Box<[u8]>),
}

impl DeltaValue {
    pub fn as_slice(&self) -> &[u8] {
        match self {
            DeltaValue::Inline { len, bytes } => &bytes[..*len as usize],
            DeltaValue::Heap(bytes) => bytes,
        }
    }

    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        match self {
            DeltaValue::Inline { len, bytes } => &mut bytes[..*len as usize],
            DeltaValue::Heap(bytes) => bytes,
        }
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.as_slice().to_vec()
    }
}

impl Default for DeltaValue {
    fn default() -> Self {
        DeltaValue::from(&[][..])
    }
}

impl Deref for DeltaValue {
    type Target = [u8];

    fn deref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl DerefMut for DeltaValue {
    fn deref_mut(&mut self) -> &mut [u8] {
        self.as_mut_slice()
    }
}

impl AsRef<[u8]> for DeltaValue {
    fn as_ref(&self) -> &[u8] {
        self.as_slice()
    }
}

impl PartialEq for DeltaValue {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl Eq for DeltaValue {}

impl PartialEq<[u8]> for DeltaValue {
    fn eq(&self, other: &[u8]) -> bool {
        self.as_slice() == other
    }
}

impl PartialEq<Vec<u8>> for DeltaValue {
    fn eq(&self, other: &Vec<u8>) -> bool {
        self.as_slice() == other.as_slice()
    }
}

impl fmt::Debug for DeltaValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_slice().fmt(f)
    }
}

impl From<&[u8]> for DeltaValue {
    fn from(value: &[u8]) -> Self {
        if value.len() <= INLINE {
            let mut bytes = [0u8; INLINE];
            bytes[..value.len()].copy_from_slice(value);
            DeltaValue::Inline {
                len: value.len() as u8,
                bytes,
            }
        } else {
            DeltaValue::Heap(value.into())
        }
    }
}

impl<const N: usize> From<[u8; N]> for DeltaValue {
    fn from(value: [u8; N]) -> Self {
        DeltaValue::from(&value[..])
    }
}

impl From<Vec<u8>> for DeltaValue {
    fn from(value: Vec<u8>) -> Self {
        if value.len() <= INLINE {
            DeltaValue::from(value.as_slice())
        } else {
            DeltaValue::Heap(value.into_boxed_slice())
        }
    }
}

impl From<&Vec<u8>> for DeltaValue {
    fn from(value: &Vec<u8>) -> Self {
        DeltaValue::from(value.as_slice())
    }
}

impl From<&DeltaValue> for DeltaValue {
    fn from(value: &DeltaValue) -> Self {
        value.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_are_inline_and_longer_values_are_not() {
        assert!(matches!(
            DeltaValue::from(1.5f32.to_le_bytes()),
            DeltaValue::Inline { len: 4, .. }
        ));
        assert!(matches!(
            DeltaValue::from(vec![7u8]),
            DeltaValue::Inline { len: 1, .. }
        ));
        let name = DeltaValue::from(b"models/player.mdl\0".to_vec());
        assert!(matches!(name, DeltaValue::Heap(_)));
        assert_eq!(&*name, b"models/player.mdl\0");
    }

    #[test]
    fn a_value_reads_back_as_the_bytes_it_was_given() {
        for bytes in [
            vec![],
            vec![1],
            vec![1, 2],
            vec![1, 2, 3, 4],
            vec![1, 2, 3, 4, 5],
        ] {
            assert_eq!(DeltaValue::from(bytes.clone()).as_slice(), &bytes[..]);
            assert_eq!(DeltaValue::from(&bytes[..]), bytes);
        }
    }

    #[test]
    fn insert_replaces_a_field_and_keeps_its_place() {
        let mut d = Delta::new();
        assert_eq!(d.insert("origin[0]\0", 1u8.to_le_bytes()), None);
        d.insert("origin[1]\0", 2u8.to_le_bytes());
        let old = d.insert("origin[0]\0".to_string(), 3u8.to_le_bytes());
        assert_eq!(old.as_deref(), Some(&[1u8][..]));
        assert_eq!(d.len(), 2);
        let keys: Vec<&str> = d.keys().map(|k| &**k).collect();
        assert_eq!(keys, ["origin[0]\0", "origin[1]\0"]);
        assert_eq!(d.get("origin[0]\0").map(|v| v.as_slice()), Some(&[3u8][..]));
        assert!(d.get("origin[0]").is_none());
    }

    #[test]
    fn a_shared_name_and_an_equal_copy_are_the_same_field() {
        let name: FieldName = "angles[1]\0".into();
        let mut d = Delta::new();
        d.insert(name.clone(), [1]);
        d.insert(FieldName::from("angles[1]\0"), [2]);
        d.insert(name, [3]);
        assert_eq!(d.len(), 1);
        assert_eq!(d.get("angles[1]\0").map(|v| v.to_vec()), Some(vec![3]));
    }

    #[test]
    fn remove_extend_and_debug() {
        let mut d: Delta = [("a", vec![1u8]), ("b", vec![2])].into_iter().collect();
        d.extend(Delta::from_iter([("b", vec![9u8]), ("c", vec![3])]));
        assert_eq!(format!("{d:?}"), r#"{"a": [1], "b": [9], "c": [3]}"#);
        assert_eq!(d.remove("a").map(|v| v.to_vec()), Some(vec![1]));
        assert!(!d.contains_key("a"));
        assert_eq!(d.len(), 2);
    }
}
