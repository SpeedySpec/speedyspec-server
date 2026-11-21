//! WordPress's PO reader and writer (`PO`): `read_entry()`, `read_line()` and `add_comment_to_entry()` as one parse
//! over a file's bytes, and `PO`'s static helpers. Each follows WordPress 7.1.5's code, not the PO format's
//! documentation, where they differ (comments do not start an entry's context; a CRLF line is read as two lines).

use speedyspec_wp_i18n_contracts::{PhpArrayKey, PoEntry};

/// What WordPress's `import_from_file()` loop reads: every entry `read_entry()` returned, in order, and whether the
/// loop ended on `false` (then `import_from_file()` returns false, keeping what it read) rather than `null`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PoParse {
    pub entries: Vec<PoEntry>,
    pub failed: bool,
}

/// PHP's `trim()` characters.
const TRIM: &[u8] = b" \t\n\r\0\x0B";

fn trim(s: &[u8]) -> &[u8] {
    let start = s.iter().position(|b| !TRIM.contains(b)).unwrap_or(s.len());
    let end = s
        .iter()
        .rposition(|b| !TRIM.contains(b))
        .map_or(start, |i| i + 1);
    &s[start..end.max(start)]
}

/// PCRE's `\s`.
fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0B' | b'\x0C' | b'\r')
}

/// `fgets()` and `PO::read_line()` over a file's bytes, with its put-back slot.
struct Lines<'a> {
    bytes: &'a [u8],
    pos: usize,
    last_line: Option<Vec<u8>>,
    use_last_line: bool,
}

impl Lines<'_> {
    fn read_line(&mut self) -> Option<Vec<u8>> {
        let line = if self.use_last_line {
            self.last_line.clone()
        } else {
            if self.pos >= self.bytes.len() {
                return None;
            }
            let rest = &self.bytes[self.pos..];
            let len = rest
                .iter()
                .position(|b| *b == b'\n')
                .map_or(rest.len(), |i| i + 1);
            let mut line = rest[..len].to_vec();
            self.pos += len;
            if let Some(r) = line.iter().position(|b| *b == b'\r') {
                // WordPress's CRLF test never holds (the \r would have to be last and be followed by \n), so every
                // \r ends the line there and the rest is read again.
                let rewind = line.len() - r - 1;
                line.truncate(r);
                line.push(b'\n');
                self.pos -= rewind;
            }
            Some(line)
        };
        self.last_line = line.clone();
        self.use_last_line = false;
        line
    }

    fn put_back(&mut self) {
        self.use_last_line = true;
    }

    fn eof(&self) -> bool {
        self.pos >= self.bytes.len()
    }
}

/// The capture of `^<keyword>\s+(".*")`: the keyword, whitespace, then from the first quote to the line's last.
fn keyword<'a>(line: &'a [u8], word: &[u8]) -> Option<&'a [u8]> {
    let rest = line.strip_prefix(word)?;
    let spaces = rest.iter().take_while(|b| is_space(**b)).count();
    if spaces == 0 {
        return None;
    }
    quoted(&rest[spaces..])
}

/// `(".*")` at the start of `s`: a quote, then up to the last quote after it.
fn quoted(s: &[u8]) -> Option<&[u8]> {
    if s.first() != Some(&b'"') {
        return None;
    }
    let last = s.iter().rposition(|b| *b == b'"')?;
    (last > 0).then(|| &s[..=last])
}

/// `^msgstr\[(\d+)\]\s+(".*")`.
fn plural_msgstr(line: &[u8]) -> Option<(&[u8], &[u8])> {
    let rest = line.strip_prefix(b"msgstr[")?;
    let digits = rest.iter().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || rest.get(digits) != Some(&b']') {
        return None;
    }
    let after = &rest[digits + 1..];
    let spaces = after.iter().take_while(|b| is_space(**b)).count();
    if spaces == 0 {
        return None;
    }
    Some((&rest[..digits], quoted(&after[spaces..])?))
}

/// The key PHP makes of a string of digits: an int when it is canonical and fits, else the string.
fn digits_key(d: &[u8]) -> PhpArrayKey {
    let text = String::from_utf8_lossy(d).into_owned();
    let canonical = d == b"0" || d.first() != Some(&b'0');
    match text.parse::<i64>() {
        Ok(n) if canonical => PhpArrayKey::Int(n),
        _ => PhpArrayKey::Str(text),
    }
}

/// `$array[$key] = $value` on an ordered list.
fn set(list: &mut Vec<(PhpArrayKey, Vec<u8>)>, key: PhpArrayKey, value: Vec<u8>) {
    match list.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1 = value,
        None => list.push((key, value)),
    }
}

/// `$array[$key] .= $more`.
fn append(list: &mut Vec<(PhpArrayKey, Vec<u8>)>, key: PhpArrayKey, more: &[u8]) {
    match list.iter_mut().find(|(k, _)| *k == key) {
        Some(slot) => slot.1.extend_from_slice(more),
        None => list.push((key, more.to_vec())),
    }
}

fn concat(field: &mut Option<Vec<u8>>, more: &[u8]) {
    field.get_or_insert_with(Vec::new).extend_from_slice(more);
}

/// `preg_split( $pattern, $s )` for `/\s+/` (`comma` false) or `/,\s*/` (`comma` true).
fn split(s: &[u8], comma: bool) -> Vec<Vec<u8>> {
    let mut out = vec![Vec::new()];
    let mut i = 0;
    while i < s.len() {
        let sep_len = if comma {
            if s[i] == b',' {
                1 + s[i + 1..].iter().take_while(|b| is_space(**b)).count()
            } else {
                0
            }
        } else {
            s[i..].iter().take_while(|b| is_space(**b)).count()
        };
        if sep_len > 0 {
            out.push(Vec::new());
            i += sep_len;
        } else {
            out.last_mut().expect("never empty").push(s[i]);
            i += 1;
        }
    }
    out
}

/// `PO::add_comment_to_entry()`.
fn add_comment(entry: &mut PoEntry, line: &[u8]) {
    let first_two = &line[..line.len().min(2)];
    let comment = trim(line.get(2..).unwrap_or(b""));
    let join = |existing: &[u8]| {
        let mut joined = existing.to_vec();
        joined.push(b'\n');
        joined.extend_from_slice(comment);
        trim(&joined).to_vec()
    };
    match first_two {
        b"#:" => entry.references.extend(split(comment, false)),
        b"#." => entry.extracted_comments = join(&entry.extracted_comments),
        b"#," => entry.flags.extend(split(comment, true)),
        _ => entry.translator_comments = join(&entry.translator_comments),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Context {
    None,
    Comment,
    Msgctxt,
    Msgid,
    MsgidPlural,
    Msgstr,
    MsgstrPlural,
}

impl Context {
    fn is_final(self) -> bool {
        matches!(self, Context::Msgstr | Context::MsgstrPlural)
    }
}

/// `PO::read_entry()`: `Ok(Some)` an entry, `Ok(None)` WordPress's null (end of file before anything), `Err` its
/// false.
fn read_entry(lines: &mut Lines<'_>) -> Result<Option<PoEntry>, ()> {
    let mut entry = PoEntry::default();
    let mut context = Context::None;
    let mut msgstr_index = PhpArrayKey::Int(0);
    loop {
        let line = lines.read_line();
        let line = match line {
            Some(l) if !l.is_empty() && l != b"0" => l,
            _ => {
                if lines.eof() {
                    if context.is_final() {
                        break;
                    } else if context == Context::None {
                        return Ok(None);
                    }
                    return Err(());
                }
                return Err(());
            }
        };
        if line == b"\n" {
            continue;
        }
        let line = trim(&line);
        if line.first() == Some(&b'#') {
            if context.is_final() {
                lines.put_back();
                break;
            }
            if context != Context::None && context != Context::Comment {
                return Err(());
            }
            add_comment(&mut entry, line);
        } else if let Some(m) = keyword(line, b"msgctxt") {
            if context.is_final() {
                lines.put_back();
                break;
            }
            if context != Context::None && context != Context::Comment {
                return Err(());
            }
            context = Context::Msgctxt;
            concat(&mut entry.context, &unpoify(m));
        } else if let Some(m) = keyword(line, b"msgid") {
            if context.is_final() {
                lines.put_back();
                break;
            }
            if !matches!(context, Context::None | Context::Msgctxt | Context::Comment) {
                return Err(());
            }
            context = Context::Msgid;
            concat(&mut entry.singular, &unpoify(m));
        } else if let Some(m) = keyword(line, b"msgid_plural") {
            if context != Context::Msgid {
                return Err(());
            }
            context = Context::MsgidPlural;
            entry.is_plural = true;
            concat(&mut entry.plural, &unpoify(m));
        } else if let Some(m) = keyword(line, b"msgstr") {
            if context != Context::Msgid {
                return Err(());
            }
            context = Context::Msgstr;
            entry.translations = vec![(PhpArrayKey::Int(0), unpoify(m))];
        } else if let Some((index, m)) = plural_msgstr(line) {
            if context != Context::MsgidPlural && context != Context::MsgstrPlural {
                return Err(());
            }
            context = Context::MsgstrPlural;
            msgstr_index = digits_key(index);
            set(&mut entry.translations, msgstr_index.clone(), unpoify(m));
        } else if line.len() >= 2 && line[0] == b'"' && line[line.len() - 1] == b'"' {
            let more = unpoify(line);
            match context {
                Context::Msgid => concat(&mut entry.singular, &more),
                Context::Msgctxt => concat(&mut entry.context, &more),
                Context::MsgidPlural => concat(&mut entry.plural, &more),
                Context::Msgstr => append(&mut entry.translations, PhpArrayKey::Int(0), &more),
                Context::MsgstrPlural => {
                    append(&mut entry.translations, msgstr_index.clone(), &more)
                }
                Context::None | Context::Comment => return Err(()),
            }
        } else {
            return Err(());
        }
    }
    if !entry.translations.iter().any(|(_, t)| !t.is_empty()) {
        entry.translations.clear();
    }
    Ok(Some(entry))
}

/// What `PO::import_from_file()` reads from a file's bytes.
pub fn parse_po(bytes: &[u8]) -> PoParse {
    let mut lines = Lines {
        bytes,
        pos: 0,
        last_line: None,
        use_last_line: false,
    };
    let mut out = PoParse::default();
    loop {
        match read_entry(&mut lines) {
            Ok(Some(entry)) => out.entries.push(entry),
            Ok(None) => return out,
            Err(()) => {
                out.failed = true;
                return out;
            }
        }
    }
}

/// `PO::poify()`.
pub fn poify(s: &[u8]) -> Vec<u8> {
    let mut escaped = Vec::with_capacity(s.len() + 8);
    for &b in s {
        match b {
            b'\\' => escaped.extend_from_slice(b"\\\\"),
            b'"' => escaped.extend_from_slice(b"\\\""),
            b'\t' => escaped.extend_from_slice(b"\\t"),
            _ => escaped.push(b),
        }
    }
    let mut po = vec![b'"'];
    for (i, part) in escaped.split(|b| *b == b'\n').enumerate() {
        if i > 0 {
            po.extend_from_slice(b"\\n\"\n\"");
        }
        po.extend_from_slice(part);
    }
    po.push(b'"');
    let newlines = escaped.iter().filter(|b| **b == b'\n').count();
    if newlines > 0 && (newlines > 1 || escaped.last() != Some(&b'\n')) {
        let mut with_empty = b"\"\"\n".to_vec();
        with_empty.extend_from_slice(&po);
        po = with_empty;
    }
    replace(&po, b"\n\"\"", b"")
}

/// `str_replace( $from, $to, $s )`.
fn replace(s: &[u8], from: &[u8], to: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i..].starts_with(from) {
            out.extend_from_slice(to);
            i += from.len();
        } else {
            out.push(s[i]);
            i += 1;
        }
    }
    out
}

/// `PO::unpoify()`.
pub fn unpoify(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    let mut previous_is_backslash = false;
    for line in s.split(|b| *b == b'\n') {
        let line = trim_quotes(trim(line));
        // preg_match_all( '/./u' ) finds no characters in a line that is not UTF-8.
        let Ok(text) = std::str::from_utf8(line) else {
            continue;
        };
        for ch in text.chars() {
            let mut buf = [0u8; 4];
            let bytes = ch.encode_utf8(&mut buf).as_bytes();
            if !previous_is_backslash {
                if ch == '\\' {
                    previous_is_backslash = true;
                } else {
                    out.extend_from_slice(bytes);
                }
            } else {
                previous_is_backslash = false;
                match ch {
                    't' => out.push(b'\t'),
                    'n' => out.push(b'\n'),
                    'r' => out.push(b'\r'),
                    '\\' => out.push(b'\\'),
                    _ => out.extend_from_slice(bytes),
                }
            }
        }
    }
    replace(&replace(&out, b"\r\n", b"\n"), b"\r", b"\n")
}

/// `PO::prepend_each_line()`.
pub fn prepend_each_line(s: &[u8], with: &[u8]) -> Vec<u8> {
    let mut lines: Vec<&[u8]> = s.split(|b| *b == b'\n').collect();
    let mut append: &[u8] = b"";
    if s.last() == Some(&b'\n') && lines.last() == Some(&&b""[..]) {
        lines.pop();
        append = b"\n";
    }
    let mut out = Vec::with_capacity(s.len() + lines.len() * with.len());
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push(b'\n');
        }
        out.extend_from_slice(with);
        out.extend_from_slice(line);
    }
    out.extend_from_slice(append);
    out
}

/// PHP's `wordwrap( $s, $width )` (break `\n`, words not cut).
fn wordwrap(s: &[u8], width: usize) -> Vec<u8> {
    let mut out = s.to_vec();
    let (mut laststart, mut lastspace) = (0usize, 0usize);
    for current in 0..s.len() {
        if s[current] == b'\n' {
            laststart = current + 1;
            lastspace = current + 1;
        } else if s[current] == b' ' {
            if current - laststart >= width {
                out[current] = b'\n';
                laststart = current + 1;
            }
            lastspace = current;
        } else if current - laststart >= width && laststart != lastspace {
            out[lastspace] = b'\n';
            laststart = lastspace + 1;
        }
    }
    out
}

/// `PO_MAX_LINE_LEN`.
const MAX_LINE_LEN: usize = 79;

/// `PO::comment_block( $text, $char )` (`$char` may be any string, as PHP allows).
pub fn comment_block(text: &[u8], ch: &[u8]) -> Vec<u8> {
    prepend_each_line(
        &wordwrap(text, MAX_LINE_LEN - 3),
        &[b"#", ch, b" "].concat(),
    )
}

/// `PO::trim_quotes()`.
pub fn trim_quotes(s: &[u8]) -> &[u8] {
    let s = s.strip_prefix(b"\"").unwrap_or(s);
    s.strip_suffix(b"\"").unwrap_or(s)
}

/// `PO::match_begin_and_end_newlines()`. As in WordPress, the translation's newline flags are read once, before
/// either end is trimmed.
pub fn match_begin_and_end_newlines(translation: &[u8], original: &[u8]) -> Vec<u8> {
    if translation.is_empty() {
        return Vec::new();
    }
    let original_begin = original.first() == Some(&b'\n');
    let original_end = original.last() == Some(&b'\n');
    let translation_begin = translation.first() == Some(&b'\n');
    let translation_end = translation.last() == Some(&b'\n');
    let mut t = translation.to_vec();
    if original_begin {
        if !translation_begin {
            t.insert(0, b'\n');
        }
    } else if translation_begin {
        let lead = t.iter().take_while(|b| **b == b'\n').count();
        t.drain(..lead);
    }
    if original_end {
        if !translation_end {
            t.push(b'\n');
        }
    } else if translation_end {
        while t.last() == Some(&b'\n') {
            t.pop();
        }
    }
    t
}

/// PHP's `empty()` for a string.
fn empty(s: &[u8]) -> bool {
    s.is_empty() || s == b"0"
}

fn key_text(k: &PhpArrayKey) -> String {
    match k {
        PhpArrayKey::Int(n) => n.to_string(),
        PhpArrayKey::Str(s) => s.clone(),
    }
}

/// `PO::export_entry()`: `None` is WordPress's false (no singular).
pub fn export_entry(entry: &PoEntry) -> Option<Vec<u8>> {
    let singular = entry.singular.as_deref().filter(|s| !s.is_empty())?;
    let mut po: Vec<Vec<u8>> = Vec::new();
    if !empty(&entry.translator_comments) {
        po.push(comment_block(&entry.translator_comments, b" "));
    }
    if !empty(&entry.extracted_comments) {
        po.push(comment_block(&entry.extracted_comments, b"."));
    }
    if !entry.references.is_empty() {
        po.push(comment_block(&entry.references.join(&b' '), b":"));
    }
    if !entry.flags.is_empty() {
        po.push(comment_block(&entry.flags.join(&b", "[..]), b","));
    }
    if let Some(context) = entry.context.as_deref().filter(|c| !empty(c)) {
        po.push([&b"msgctxt "[..], &poify(context)].concat());
    }
    po.push([&b"msgid "[..], &poify(singular)].concat());
    if !entry.is_plural {
        let translation = entry
            .translations
            .iter()
            .find(|(k, _)| *k == PhpArrayKey::Int(0))
            .map(|(_, t)| t.as_slice())
            .unwrap_or(b"");
        let translation = match_begin_and_end_newlines(translation, singular);
        po.push([&b"msgstr "[..], &poify(&translation)].concat());
    } else {
        let plural = entry.plural.as_deref().unwrap_or(b"");
        po.push([&b"msgid_plural "[..], &poify(plural)].concat());
        let defaults = vec![
            (PhpArrayKey::Int(0), Vec::new()),
            (PhpArrayKey::Int(1), Vec::new()),
        ];
        let translations = if entry.translations.is_empty() {
            &defaults
        } else {
            &entry.translations
        };
        for (key, translation) in translations {
            let translation = match_begin_and_end_newlines(translation, plural);
            po.push(
                [
                    format!("msgstr[{}] ", key_text(key)).as_bytes(),
                    &poify(&translation),
                ]
                .concat(),
            );
        }
    }
    Some(po.join(&b'\n'))
}
