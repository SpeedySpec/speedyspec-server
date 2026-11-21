//! The cache's key layout in its keyspace.

/// The group's generation counter: `b"g"` then the group; absent is generation 0.
pub fn generation(group: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(1 + group.len());
    out.push(b'g');
    out.extend(group);
    out
}

/// An entry: `b"e"`, the group's length (4 bytes big-endian), the group, the generation (8 bytes big-endian), the
/// key. The length makes group and key unambiguous.
pub fn entry(group: &[u8], generation: u64, key: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(13 + group.len() + key.len());
    out.push(b'e');
    out.extend((group.len() as u32).to_be_bytes());
    out.extend(group);
    out.extend(generation.to_be_bytes());
    out.extend(key);
    out
}
