//! Reading `theme.json` files: `wp_json_file_decode( $file, array( 'associative' => true ) )`, kept per worker by
//! path, size and modification time, so a file changed on disk is read again.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::SystemTime;

use speedyspec_hooks::Value;
use speedyspec_wp_theme_json_contracts::ports::{Diagnostics, Gettext};

use crate::php_json;

/// A decoded file with the size and modification time it had when read.
type Stamped = (u64, Option<SystemTime>, Value);

/// The decoded files this worker has read, by path.
#[derive(Default)]
pub struct FileCache {
    files: RefCell<HashMap<String, Stamped>>,
}

/// `wp_normalize_path( realpath( $file ) )`, or `None` when the file does not exist.
fn real_path(file: &str) -> Option<String> {
    std::fs::canonicalize(file)
        .ok()
        .map(|p| p.to_string_lossy().replace('\\', "/"))
}

impl FileCache {
    /// `read_json_file( $file_path )`: the decoded array, or an empty array when the file is missing or invalid
    /// (with WordPress's notice).
    pub fn read(&self, file: &str, diagnostics: &dyn Diagnostics, gettext: &dyn Gettext) -> Value {
        if file.is_empty() {
            return Value::Array(Vec::new());
        }
        let meta = std::fs::metadata(file).ok();
        let stamp = meta.as_ref().map(|m| (m.len(), m.modified().ok()));
        if let Some((size, modified)) = stamp
            && let Some((s, m, value)) = self.files.borrow().get(file)
            && *s == size
            && *m == modified
        {
            return value.clone();
        }
        match json_file_decode(file, diagnostics, gettext) {
            Some(value @ Value::Array(_)) => {
                if let Some((size, modified)) = stamp {
                    self.files
                        .borrow_mut()
                        .insert(file.to_owned(), (size, modified, value.clone()));
                }
                value
            }
            _ => Value::Array(Vec::new()),
        }
    }

    /// Forgets every file (tests; WordPress's cache is per request, this one per worker).
    pub fn clear(&self) {
        self.files.borrow_mut().clear();
    }
}

/// `wp_json_file_decode( $filename, array( 'associative' => true ) )`.
pub fn json_file_decode(
    file: &str,
    diagnostics: &dyn Diagnostics,
    gettext: &dyn Gettext,
) -> Option<Value> {
    let Some(path) = real_path(file) else {
        let template = gettext.translate("File %s doesn't exist!", "default");
        let message = String::from_utf8_lossy(&template).replacen("%s", "", 1);
        diagnostics.trigger_error("wp_json_file_decode", message.as_bytes());
        return None;
    };
    let bytes = std::fs::read(&path).unwrap_or_default();
    match php_json::decode(&bytes) {
        Ok(value) => Some(value),
        Err(e) => {
            let template = gettext.translate(
                "Error when decoding a JSON file at path %1$s: %2$s",
                "default",
            );
            let message = String::from_utf8_lossy(&template)
                .replacen("%1$s", &path, 1)
                .replacen("%2$s", e.0, 1);
            diagnostics.trigger_error("wp_json_file_decode", message.as_bytes());
            None
        }
    }
}

/// `is_readable( $file )`.
pub fn is_readable(file: &str) -> bool {
    !file.is_empty() && std::fs::File::open(file).is_ok()
}
