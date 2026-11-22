//! A plain ordered tree: how a reader's raw view (FFmpeg's, getID3's shape) travels without either library's types.

/// A value in a raw view or a WordPress array. Maps keep their order and may repeat keys, as the libraries' own
/// structures do. PHP strings in WordPress arrays are `Bytes` (they need not be UTF-8); `Str` appears only in readers'
/// raw views.
#[derive(Clone, Debug, PartialEq)]
pub enum Tree {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    Bytes(Vec<u8>),
    List(Vec<Tree>),
    Map(Vec<(String, Tree)>),
}

impl Tree {
    /// The first value under `key` when this is a map.
    pub fn get(&self, key: &str) -> Option<&Tree> {
        match self {
            Tree::Map(entries) => entries
                .iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// The bytes of a string, whichever kind it is.
    pub fn bytes(&self) -> Option<&[u8]> {
        match self {
            Tree::Bytes(b) => Some(b),
            Tree::Str(s) => Some(s.as_bytes()),
            _ => None,
        }
    }
}
