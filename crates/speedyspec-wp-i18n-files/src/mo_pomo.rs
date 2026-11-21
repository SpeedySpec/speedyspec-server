//! MO files as pomo reads and writes them (`MO::import_from_reader()`, `MO::export_to_file_handle()`), and the
//! bytes `WP_Translation_File_MO::export()` and `WP_Translation_File_PHP::export()` produce. Each follows WordPress
//! 7.1.5's code, PHP's quirks included (`substr()` with offsets past either end, `array_merge()` renumbering
//! integer keys, `var_export()`'s escaping).

/// What `MO::import_from_reader()` reads from a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MoPairs {
    /// Every `(original, translation)` pair in file order; the header pair's original is empty (WordPress hands
    /// its translation to `make_headers()`, the others to `make_entry()`).
    Pairs(Vec<(Vec<u8>, Vec<u8>)>),
    /// WordPress returns false (magic, revision, or tables that do not fit).
    False,
    /// A table of zero bytes: WordPress's reader calls `fread()` with length 0, which throws a `ValueError` on PHP 8.
    ZeroLengthRead,
}

const MAGIC: u32 = 0x950412de;

/// A file read as `POMO_FileReader` reads it: a position that may pass the end.
struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    /// `fread()`: up to `n` bytes from the position.
    fn read(&mut self, n: usize) -> &[u8] {
        let start = self.pos.min(self.bytes.len());
        let end = start.saturating_add(n).min(self.bytes.len());
        self.pos = self.pos.saturating_add(end - start);
        &self.bytes[start..end]
    }

    /// `stream_get_contents()`: everything from the position.
    fn read_all(&mut self) -> &[u8] {
        let start = self.pos.min(self.bytes.len());
        self.pos = self.bytes.len().max(self.pos);
        &self.bytes[start..]
    }
}

fn u32_at(b: &[u8], big: bool) -> i64 {
    let a = [b[0], b[1], b[2], b[3]];
    i64::from(if big {
        u32::from_be_bytes(a)
    } else {
        u32::from_le_bytes(a)
    })
}

/// PHP's `substr( $s, $start, $length )` for a non-negative length.
fn substr(s: &[u8], start: i64, length: i64) -> &[u8] {
    let len = s.len() as i64;
    let start = if start < 0 {
        (len + start).max(0)
    } else {
        start
    };
    if start > len {
        return b"";
    }
    let end = start.saturating_add(length).min(len);
    &s[start as usize..end.max(start) as usize]
}

/// `MO::import_from_reader( new POMO_FileReader( $file ) )` over a file's bytes.
pub fn mo_pairs(bytes: &[u8]) -> MoPairs {
    let mut r = Reader { bytes, pos: 0 };
    let magic = r.read(4);
    if magic.len() != 4 {
        return MoPairs::False;
    }
    let big = match u32_at(magic, false) as u32 {
        MAGIC => false,
        m if m == MAGIC.swap_bytes() => true,
        _ => return MoPairs::False,
    };
    let header = r.read(24);
    if header.len() != 24 {
        return MoPairs::False;
    }
    let field = |i: usize| u32_at(&header[i * 4..], big);
    let (revision, total, originals_addr, translations_addr, hash_length, hash_addr) =
        (field(0), field(1), field(2), field(3), field(4), field(5));
    if revision != 0 {
        return MoPairs::False;
    }
    r.pos = originals_addr as usize;
    let originals_length = translations_addr - originals_addr;
    if originals_length != total * 8 {
        return MoPairs::False;
    }
    if originals_length == 0 {
        return MoPairs::ZeroLengthRead;
    }
    let originals = r.read(originals_length as usize).to_vec();
    if originals.len() as i64 != originals_length {
        return MoPairs::False;
    }
    let translations_length = hash_addr - translations_addr;
    if translations_length != total * 8 {
        return MoPairs::False;
    }
    let translations = r.read(translations_length as usize).to_vec();
    if translations.len() as i64 != translations_length {
        return MoPairs::False;
    }
    let strings_addr = hash_addr + hash_length * 4;
    r.pos = strings_addr as usize;
    let strings = r.read_all();
    let mut pairs = Vec::with_capacity(total as usize);
    for i in 0..total as usize {
        let o = &originals[i * 8..i * 8 + 8];
        let t = &translations[i * 8..i * 8 + 8];
        let original = substr(strings, u32_at(&o[4..], big) - strings_addr, u32_at(o, big));
        let translation = substr(strings, u32_at(&t[4..], big) - strings_addr, u32_at(t, big));
        pairs.push((original.to_vec(), translation.to_vec()));
    }
    MoPairs::Pairs(pairs)
}

fn pack(out: &mut Vec<u8>, values: &[u32], big: bool) {
    for v in values {
        out.extend_from_slice(&if big {
            v.to_be_bytes()
        } else {
            v.to_le_bytes()
        });
    }
}

/// The bytes `MO::export_to_file_handle()` writes, given its entries' `export_original()` and
/// `export_translations()` strings (already filtered and in `ksort` order) and `export_headers()`.
pub fn mo_pack(pairs: &[(Vec<u8>, Vec<u8>)], headers: &[u8]) -> Vec<u8> {
    let total = pairs.len() as u32 + 1;
    let originals_addr = 28u32;
    let translations_addr = originals_addr + 8 * total;
    let hash_addr = translations_addr + 8 * total;
    let mut out = Vec::new();
    pack(
        &mut out,
        &[
            MAGIC,
            0,
            total,
            originals_addr,
            translations_addr,
            0,
            hash_addr,
        ],
        false,
    );
    let mut current = hash_addr;
    pack(&mut out, &[0, current], false);
    current += 1;
    let mut originals_table = vec![0u8];
    for (original, _) in pairs {
        originals_table.extend_from_slice(original);
        originals_table.push(0);
        pack(&mut out, &[original.len() as u32, current], false);
        current += original.len() as u32 + 1;
    }
    pack(&mut out, &[headers.len() as u32, current], false);
    current += headers.len() as u32 + 1;
    let mut translations_table = headers.to_vec();
    translations_table.push(0);
    for (_, translation) in pairs {
        translations_table.extend_from_slice(translation);
        translations_table.push(0);
        pack(&mut out, &[translation.len() as u32, current], false);
        current += translation.len() as u32 + 1;
    }
    out.extend_from_slice(&originals_table);
    out.extend_from_slice(&translations_table);
    out
}

/// Whether PHP makes `key` an int array key: a canonical decimal integer that fits.
fn int_key(key: &[u8]) -> Option<i64> {
    let digits = key.strip_prefix(b"-").unwrap_or(key);
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if (digits[0] == b'0' && digits.len() > 1) || key == b"-0" {
        return None;
    }
    std::str::from_utf8(key).ok()?.parse().ok()
}

/// `array_merge()` of arrays given as `(key, value)`: string keys kept, integer keys renumbered from 0. A repeated
/// key keeps its first position and takes the last value; keys are found through an index, so this is linear.
fn merge<V: Clone>(parts: &[&[(Vec<u8>, V)]]) -> Vec<(Vec<u8>, V)> {
    let mut out: Vec<(Vec<u8>, V)> = Vec::new();
    let mut index: std::collections::HashMap<Vec<u8>, usize> = std::collections::HashMap::new();
    let mut next = 0i64;
    for part in parts {
        for (k, v) in part.iter() {
            let key = match int_key(k) {
                Some(_) => {
                    let n = next.to_string().into_bytes();
                    next += 1;
                    n
                }
                None => k.clone(),
            };
            match index.get(&key) {
                Some(&i) => out[i].1 = v.clone(),
                None => {
                    index.insert(key.clone(), out.len());
                    out.push((key, v.clone()));
                }
            }
        }
    }
    out
}

/// `WP_Translation_File_MO::export()`: the headers as one `''` entry, then the entries (`array_merge()`d, so integer
/// keys are renumbered), packed little-endian unless the file was big-endian.
pub fn translation_file_mo(
    headers: &[(Vec<u8>, Vec<u8>)],
    entries: &[(Vec<u8>, Vec<u8>)],
    big_endian: bool,
) -> Vec<u8> {
    let mut header_text = Vec::new();
    for (name, value) in headers {
        header_text.extend_from_slice(name);
        header_text.extend_from_slice(b": ");
        header_text.extend_from_slice(value);
        header_text.push(b'\n');
    }
    let first = [(Vec::new(), header_text)];
    let all = merge(&[&first[..], entries]);
    let count = all.len() as u32;
    let bytes_for_entries = count * 4 * 2;
    let originals_addr = 28u32;
    let translations_addr = originals_addr + bytes_for_entries;
    let hash_addr = translations_addr + bytes_for_entries;
    let mut offset = hash_addr;
    let mut out = Vec::new();
    pack(
        &mut out,
        &[
            MAGIC,
            0,
            count,
            originals_addr,
            translations_addr,
            0,
            hash_addr,
        ],
        big_endian,
    );
    let mut o_entries = Vec::new();
    for (original, _) in &all {
        pack(&mut out, &[original.len() as u32, offset], big_endian);
        offset += original.len() as u32 + 1;
        o_entries.extend_from_slice(original);
        o_entries.push(0);
    }
    let mut t_entries = Vec::new();
    for (_, translation) in &all {
        pack(&mut out, &[translation.len() as u32, offset], big_endian);
        offset += translation.len() as u32 + 1;
        t_entries.extend_from_slice(translation);
        t_entries.push(0);
    }
    out.extend_from_slice(&o_entries);
    out.extend_from_slice(&t_entries);
    out
}

/// PHP's `var_export()` of a string: single-quoted, `'` and `\` escaped, each NUL byte as `' . "\0" . '`.
pub fn var_export_string(s: &[u8]) -> Vec<u8> {
    let mut out = vec![b'\''];
    for &b in s {
        match b {
            b'\'' | b'\\' => {
                out.push(b'\\');
                out.push(b);
            }
            0 => out.extend_from_slice(b"' . \"\\0\" . '"),
            _ => out.push(b),
        }
    }
    out.push(b'\'');
    out
}

/// `var_export()` of an array key: an int key bare, a string key quoted.
fn var_export_key(k: &[u8]) -> Vec<u8> {
    match int_key(k) {
        Some(n) => n.to_string().into_bytes(),
        None => var_export_string(k),
    }
}

/// `WP_Translation_File_PHP::var_export()` of an array of strings: a list without keys, else `key=>value`.
fn export_strings(items: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let is_list = items
        .iter()
        .enumerate()
        .all(|(i, (k, _))| int_key(k) == Some(i as i64));
    let parts: Vec<Vec<u8>> = items
        .iter()
        .map(|(k, v)| {
            if is_list {
                var_export_string(v)
            } else {
                [var_export_key(k), b"=>".to_vec(), var_export_string(v)].concat()
            }
        })
        .collect();
    [b"[".to_vec(), parts.join(&b','), b"]".to_vec()].concat()
}

/// `WP_Translation_File_PHP::export()`: `array_merge( $headers, ['messages' => $entries] )` as a PHP file.
pub fn translation_file_php(
    headers: &[(Vec<u8>, Vec<u8>)],
    entries: &[(Vec<u8>, Vec<u8>)],
) -> Vec<u8> {
    /// A value of the exported array: a header's string, or the messages.
    #[derive(Clone)]
    enum Item {
        Str(Vec<u8>),
        Messages,
    }
    let headers: Vec<(Vec<u8>, Item)> = headers
        .iter()
        .map(|(k, v)| (k.clone(), Item::Str(v.clone())))
        .collect();
    let messages = [(b"messages".to_vec(), Item::Messages)];
    let data = merge(&[&headers[..], &messages[..]]);
    let is_list = data
        .iter()
        .enumerate()
        .all(|(i, (k, _))| int_key(k) == Some(i as i64));
    let parts: Vec<Vec<u8>> = data
        .iter()
        .map(|(k, v)| {
            let value = match v {
                Item::Str(s) => var_export_string(s),
                Item::Messages => export_strings(entries),
            };
            if is_list {
                value
            } else {
                [var_export_key(k), b"=>".to_vec(), value].concat()
            }
        })
        .collect();
    [
        b"<?php\nreturn [".to_vec(),
        parts.join(&b','),
        b"];\n".to_vec(),
    ]
    .concat()
}
