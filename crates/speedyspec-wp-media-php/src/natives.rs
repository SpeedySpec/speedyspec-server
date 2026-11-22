//! The native functions the facade calls, in `SpeedySpec\WpMedia\Native`.

use std::path::Path;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_hooks::{Count, HookApi, HookError, HookName, HookValue as Value};
use speedyspec_wp_media::functions::{
    Outcome, add_id3_tag_data, media_creation_timestamp, read_audio_metadata, read_video_metadata,
};
use speedyspec_wp_media::neutral::neutral;
use speedyspec_wp_media_contracts::{MEDIA_READER, MediaReader, MediaSource, ProbeLimits, Tree};

use crate::adapters::{Pending, PhpTagSanitizer, PhpTimeParser, TooLarge};
use crate::convert::{from_php, to_php};

pub(crate) struct Natives {
    pub hooks: Rc<HookApi>,
    pub container: Rc<Container>,
    pub limits: ProbeLimits,
    pub default: std::cell::RefCell<Option<Rc<dyn MediaReader>>>,
    pub pending: Pending,
}

fn hook_error(err: HookError) -> NativeError {
    match &err.payload {
        Some(payload) => match payload.downcast_ref::<Value>() {
            Some(exception) => NativeError::Rethrow(exception.clone()),
            None => NativeError::Silent,
        },
        None => NativeError::throw("Error", err.message),
    }
}

fn bytes(value: Option<&Value>) -> Vec<u8> {
    value
        .map(from_php)
        .and_then(|t| t.bytes().map(<[u8]>::to_vec))
        .unwrap_or_default()
}

fn path_of(bytes: &[u8]) -> &Path {
    Path::new(<std::ffi::OsStr as std::os::unix::ffi::OsStrExt>::from_bytes(bytes))
}

impl Natives {
    fn reader(&self) -> Result<Rc<dyn MediaReader>, NativeError> {
        self.container
            .make_port::<dyn MediaReader>(MEDIA_READER)
            .map_err(|err| NativeError::throw("Error", err.to_string()))
    }

    fn rethrow_pending(&self) -> Result<(), NativeError> {
        match self.pending.take() {
            Some(exception) => Err(NativeError::Rethrow(exception)),
            None => Ok(()),
        }
    }

    /// `wp_read_{kind}_metadata($original)` reading `local` (a local copy for a stream-wrapper path).
    fn read_metadata(&self, args: &[Value]) -> Result<Value, NativeError> {
        let kind = bytes(args.first());
        let local = bytes(args.get(1));
        let original = args.get(2).cloned().unwrap_or(Value::Null);
        let too_large = matches!(args.get(3), Some(Value::Bool(true)));
        let sanitizer = PhpTagSanitizer {
            pending: self.pending.clone(),
        };
        let reader: Rc<dyn MediaReader> = if too_large {
            Rc::new(TooLarge)
        } else {
            self.reader()?
        };
        let read = if kind == b"video" {
            read_video_metadata
        } else {
            read_audio_metadata
        };
        let outcome = read(
            path_of(&local),
            reader.as_ref(),
            &self.limits,
            &sanitizer,
            &PhpTimeParser,
        );
        self.rethrow_pending()?;
        let Some(Outcome {
            metadata,
            file_format,
            data,
        }) = outcome
        else {
            return Ok(Value::Bool(false));
        };
        let name = HookName::from(if kind == b"video" {
            "wp_read_video_metadata"
        } else {
            "wp_read_audio_metadata"
        });
        let value = to_php(&metadata);
        if self
            .hooks
            .skip_if_empty(&name, Count::Filters)
            .map_err(hook_error)?
        {
            return Ok(value);
        }
        let format = file_format
            .map(|f| to_php(&Tree::Bytes(f)))
            .unwrap_or(Value::Null);
        self.hooks
            .apply_filters(&name, value, vec![original, format, to_php(&data)])
            .map_err(hook_error)
    }

    /// The keys `wp_add_id3_tag_data()` sets from `$data`, in order (the facade assigns them into `$metadata`).
    fn id3_tag_changes(&self, args: &[Value]) -> Result<Value, NativeError> {
        let mut metadata = Tree::Map(Vec::new());
        let data = args.first().map(from_php).unwrap_or(Tree::Null);
        add_id3_tag_data(
            &mut metadata,
            &data,
            &PhpTagSanitizer {
                pending: self.pending.clone(),
            },
        );
        self.rethrow_pending()?;
        Ok(to_php(&metadata))
    }

    fn creation_timestamp(&self, args: &[Value]) -> Result<Value, NativeError> {
        let data = args.first().map(from_php).unwrap_or(Tree::Null);
        Ok(media_creation_timestamp(&data, &PhpTimeParser).map_or(Value::Bool(false), Value::Int))
    }

    fn read_media(&self, args: &[Value]) -> Result<Value, NativeError> {
        let Some(reader) = self.default.borrow().clone() else {
            return Ok(Value::Bool(false));
        };
        let path = bytes(args.first());
        Ok(
            match reader.read(
                MediaSource::Path(path_of(&path).to_path_buf()),
                &self.limits,
            ) {
                Ok(info) => to_php(&neutral(&info)),
                Err(_) => Value::Bool(false),
            },
        )
    }
}

pub(crate) fn functions(natives: &Rc<Natives>) -> Vec<FunctionBuilder<'static>> {
    let ns = "SpeedySpec\\WpMedia\\Native\\";
    let n = natives.clone();
    let read = native::function(&format!("{ns}read_metadata"), move |args| {
        n.read_metadata(args)
    });
    let n = natives.clone();
    let id3 = native::function(&format!("{ns}id3_tag_changes"), move |args| {
        n.id3_tag_changes(args)
    });
    let n = natives.clone();
    let ts = native::function(
        &format!("{ns}wp_get_media_creation_timestamp"),
        move |args| n.creation_timestamp(args),
    );
    let n = natives.clone();
    let media = native::function(&format!("{ns}read_media"), move |args| n.read_media(args));
    let max = natives.limits.max_bytes as i64;
    let max_bytes = native::function(&format!("{ns}max_bytes"), move |_| Ok(Value::Int(max)));
    vec![read, id3, ts, media, max_bytes]
}
