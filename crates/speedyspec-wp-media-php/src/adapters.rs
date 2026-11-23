//! The ports WordPress's media rules need, over PHP: kses, `strtotime`, fileinfo, and a PHP object replacing the
//! reader.

use std::cell::RefCell;

use std::path::Path;
use std::rc::Rc;

use speedyspec_hooks::Key;
use speedyspec_php::{CallFailure, PhpError};
use speedyspec_php_ext::PhpObject;
use speedyspec_wp_hooks::HookValue as Value;
use speedyspec_wp_media::neutral::from_neutral;
use speedyspec_wp_media_contracts::{
    ContentSniffer, MediaInfo, MediaReader, MediaSource, ProbeLimits, ReadError, TagSanitizer,
    TimeParser, Tree,
};

use crate::convert::{from_php, to_php};

/// An exception a PHP call threw, kept until the native function that started the call can rethrow it.
#[derive(Clone, Default)]
pub struct Pending(Rc<RefCell<Option<Value>>>);

impl Pending {
    pub fn keep(&self, failure: &CallFailure) {
        if let Some(exception) = &failure.exception {
            *self.0.borrow_mut() = Some(exception.clone());
        }
    }
    pub fn take(&self) -> Option<Value> {
        self.0.borrow_mut().take()
    }
}

fn describe(failure: &CallFailure) -> String {
    match &failure.error {
        PhpError::Exit(_) => "PHP exited".into(),
        other => other.to_string(),
    }
}

/// `wp_kses_post()` and `wp_kses_post_deep()` through PHP (the kses port's natives answer them in the runtime).
pub struct PhpTagSanitizer {
    pub pending: Pending,
}

impl TagSanitizer for PhpTagSanitizer {
    fn post(&self, value: &[u8]) -> Vec<u8> {
        let arg = to_php(&Tree::Bytes(value.to_vec()));
        match speedyspec_php::call_catching(&Value::Str("wp_kses_post".into()), &[arg]) {
            Ok(out) => from_php(&out)
                .bytes()
                .map(<[u8]>::to_vec)
                .unwrap_or_default(),
            Err(failure) => {
                self.pending.keep(&failure);
                value.to_vec()
            }
        }
    }

    fn post_deep(&self, value: &Tree) -> Tree {
        match speedyspec_php::call_catching(
            &Value::Str("wp_kses_post_deep".into()),
            &[to_php(value)],
        ) {
            Ok(out) => from_php(&out),
            Err(failure) => {
                self.pending.keep(&failure);
                value.clone()
            }
        }
    }
}

/// PHP's `strtotime()`.
pub struct PhpTimeParser;

impl TimeParser for PhpTimeParser {
    fn strtotime(&self, text: &[u8]) -> Option<i64> {
        let arg = to_php(&Tree::Bytes(text.to_vec()));
        match speedyspec_php::call_catching(&Value::Str("strtotime".into()), &[arg]) {
            Ok(Value::Int(t)) => Some(t),
            _ => None,
        }
    }
}

/// A PHP object bound to `MEDIA_READER` in place of the default reader (Adapter). A thrown exception is kept for the
/// native function to rethrow; anything but the neutral array or `false` is malformed.
pub struct PhpReplacement {
    object: PhpObject,
    pending: Pending,
}

impl PhpReplacement {
    pub fn new(object: PhpObject, pending: Pending) -> PhpReplacement {
        PhpReplacement { object, pending }
    }
}

impl MediaReader for PhpReplacement {
    fn read(&self, source: MediaSource, _limits: &ProbeLimits) -> Result<MediaInfo, ReadError> {
        let MediaSource::Path(path) = source else {
            return Err(ReadError::Crashed("a PHP reader reads paths only".into()));
        };
        let callable = Value::Array(vec![
            (Key::Int(0), self.object.value().clone()),
            (Key::Int(1), Value::Str("read".into())),
        ]);
        let arg = to_php(&Tree::Bytes(path.as_os_str().as_encoded_bytes().to_vec()));
        match speedyspec_php::call_catching(&callable, &[arg]) {
            Ok(value) => match from_php(&value) {
                Tree::Bool(false) => Err(ReadError::Unrecognized),
                tree => from_neutral(&tree),
            },
            Err(failure) => {
                self.pending.keep(&failure);
                Err(ReadError::Crashed(describe(&failure)))
            }
        }
    }
}

/// A reader for a file the facade couldn't copy locally within the byte limit.
pub struct TooLarge;

impl MediaReader for TooLarge {
    fn read(&self, _: MediaSource, _: &ProbeLimits) -> Result<MediaInfo, ReadError> {
        Err(ReadError::LimitExceeded(
            speedyspec_wp_media_contracts::Limit::Bytes,
        ))
    }
}

/// PHP's fileinfo (`finfo_file()` with `FILEINFO_MIME_TYPE`), the content check WordPress makes (Adapter). A thrown
/// exception is kept for the native function to rethrow.
pub struct PhpFileinfo {
    pub pending: Pending,
    /// The request's `finfo` handle, opened on first use: opening one loads the magic database.
    pub handle: FileinfoHandle,
}

/// A request's `finfo` handle, shared by every check in it. PHP frees it with the request, so the area forgets it at
/// each request's start.
#[derive(Clone, Default)]
pub struct FileinfoHandle(Rc<RefCell<Option<Value>>>);

impl FileinfoHandle {
    /// Forgets the handle (the request that opened it has ended).
    pub fn clear(&self) {
        self.0.borrow_mut().take();
    }
}

/// PHP's `FILEINFO_MIME_TYPE`.
const FILEINFO_MIME_TYPE: i64 = 16;

impl PhpFileinfo {
    fn finfo(&self) -> Option<Value> {
        if let Some(finfo) = self.handle.0.borrow().clone() {
            return Some(finfo);
        }
        match speedyspec_php::call_catching(
            &Value::Str("finfo_open".into()),
            &[Value::Int(FILEINFO_MIME_TYPE)],
        ) {
            Ok(Value::Bool(false)) => None,
            Ok(finfo) => {
                *self.handle.0.borrow_mut() = Some(finfo.clone());
                Some(finfo)
            }
            Err(failure) => {
                self.pending.keep(&failure);
                None
            }
        }
    }
}

impl ContentSniffer for PhpFileinfo {
    fn sniff(&self, path: &Path) -> Option<String> {
        let finfo = self.finfo()?;
        let arg = to_php(&Tree::Bytes(path.as_os_str().as_encoded_bytes().to_vec()));
        match speedyspec_php::call_catching(&Value::Str("finfo_file".into()), &[finfo, arg]) {
            Ok(Value::Str(mime)) => Some(mime),
            Ok(_) => None,
            Err(failure) => {
                self.pending.keep(&failure);
                None
            }
        }
    }
}
