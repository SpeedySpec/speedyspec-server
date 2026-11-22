//! A plain ordered tree: how a reader's raw view (FFmpeg's, getID3's shape) travels without either library's types.

/// A value in a raw view. Maps keep their order and may repeat keys, as the libraries' own structures do.
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
}
