//! The native functions the facade calls, in `SpeedySpec\WpMedia\Native`.

use std::path::Path;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_php::native::{self, NativeError};
use speedyspec_wp_hooks::HookApi;
use speedyspec_wp_hooks_contracts::{Count, HookError, HookName, HookValue as Value};
use speedyspec_wp_media::functions::{
    Outcome, add_id3_tag_data, media_creation_timestamp, read_audio_metadata, read_video_metadata,
};
use speedyspec_wp_media::images;
use speedyspec_wp_media::mime;
use speedyspec_wp_media::neutral::neutral;
use speedyspec_wp_media_contracts::{MEDIA_READER, MediaReader, MediaSource, ProbeLimits, Tree};

use crate::adapters::{
    FileinfoHandle, Pending, PhpFileinfo, PhpTagSanitizer, PhpTimeParser, TooLarge,
};
use crate::convert::{from_php, to_php};

pub(crate) struct Natives {
    pub hooks: Rc<HookApi>,
    pub container: Rc<Container>,
    pub limits: ProbeLimits,
    pub default: std::cell::RefCell<Option<Rc<dyn MediaReader>>>,
    pub pending: Pending,
    pub fileinfo: FileinfoHandle,
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

    /// The image type of a file from its first bytes (read by the facade, so stream wrappers work too).
    fn image_type(&self, args: &[Value]) -> Result<Value, NativeError> {
        let head = bytes(args.first());
        Ok(match images::sniff(&head) {
            Some(t) => to_php(&Tree::Map(vec![
                ("mime".into(), Tree::Bytes(t.mime.as_bytes().to_vec())),
                ("imagetype".into(), Tree::Int(t.imagetype)),
                ("fast".into(), Tree::Bool(t.path == images::ImagePath::Fast)),
            ])),
            None => Value::Bool(false),
        })
    }

    /// `wp_getimagesize()`'s array for a reader-path type (a local file), or `false`.
    fn read_image_size(&self, args: &[Value]) -> Result<Value, NativeError> {
        let path = bytes(args.first());
        let imagetype = match args.get(1) {
            Some(Value::Int(i)) => *i,
            _ => 0,
        };
        let mime = String::from_utf8_lossy(&bytes(args.get(2))).into_owned();
        let read = self.reader()?.read(
            MediaSource::Path(path_of(&path).to_path_buf()),
            &self.limits,
        );
        self.rethrow_pending()?;
        Ok(read
            .ok()
            .and_then(|info| images::reader_imagesize(&info, imagetype, &mime))
            .map_or(Value::Bool(false), |t| to_php(&t)))
    }

    /// `wp_get_webp_info()` over the first 40 bytes the facade read.
    fn webp_info(&self, args: &[Value]) -> Result<Value, NativeError> {
        Ok(to_php(&images::webp_info(&bytes(args.first()))))
    }

    /// `wp_get_avif_info()` through the reader (a local file).
    fn avif_info(&self, args: &[Value]) -> Result<Value, NativeError> {
        let path = bytes(args.first());
        let read = self.reader()?.read(
            MediaSource::Path(path_of(&path).to_path_buf()),
            &self.limits,
        );
        self.rethrow_pending()?;
        let info = read.unwrap_or_else(|_| speedyspec_wp_media_contracts::MediaInfo {
            container: speedyspec_wp_media_contracts::ContainerKind::Other(String::new()),
            mime_type: None,
            duration: None,
            bitrate: None,
            file_size: None,
            created_at: None,
            streams: Vec::new(),
            tags: Vec::new(),
            pictures: Vec::new(),
            raw: Default::default(),
        });
        Ok(to_php(&images::avif_info(&info)))
    }

    /// The MIME type `wp_check_filetype_and_ext()` compares with the claimed `$type`: fileinfo's, confirmed by the
    /// reader when fileinfo files audio as video or video as audio (a local file).
    fn content_mime(&self, args: &[Value]) -> Result<Value, NativeError> {
        let path = bytes(args.first());
        let claimed = String::from_utf8_lossy(&bytes(args.get(1))).into_owned();
        let fileinfo = PhpFileinfo {
            pending: self.pending.clone(),
            handle: self.fileinfo.clone(),
        };
        let found = mime::content_mime(
            path_of(&path),
            &claimed,
            &fileinfo,
            &LazyReader(self),
            &self.limits,
        );
        match found {
            // Fileinfo answered, so anything pending is the reader's: a reader that fails, or can't be resolved,
            // leaves fileinfo's answer standing rather than failing the upload check.
            Some(mime) => {
                self.pending.take();
                Ok(Value::Str(mime))
            }
            // Fileinfo itself failed; the reader was never consulted.
            None => {
                self.rethrow_pending()?;
                Ok(Value::Bool(false))
            }
        }
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

/// The `MEDIA_READER` port resolved only when a read is asked for (Virtual Proxy): most filetype checks never need
/// a reader, and one that can't be resolved reads as a crash, which the content rule answers with fileinfo's type.
struct LazyReader<'a>(&'a Natives);

impl MediaReader for LazyReader<'_> {
    fn read(
        &self,
        source: MediaSource,
        limits: &ProbeLimits,
    ) -> Result<speedyspec_wp_media_contracts::MediaInfo, speedyspec_wp_media_contracts::ReadError>
    {
        match self.0.container.make_port::<dyn MediaReader>(MEDIA_READER) {
            Ok(reader) => reader.read(source, limits),
            Err(err) => Err(speedyspec_wp_media_contracts::ReadError::Crashed(
                err.to_string(),
            )),
        }
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
    let mut out = vec![read, id3, ts, media, max_bytes];
    let n = natives.clone();
    out.push(native::function(&format!("{ns}image_type"), move |args| {
        n.image_type(args)
    }));
    let n = natives.clone();
    out.push(native::function(
        &format!("{ns}read_image_size"),
        move |args| n.read_image_size(args),
    ));
    let n = natives.clone();
    out.push(native::function(&format!("{ns}webp_info"), move |args| {
        n.webp_info(args)
    }));
    let n = natives.clone();
    out.push(native::function(&format!("{ns}avif_info"), move |args| {
        n.avif_info(args)
    }));
    let n = natives.clone();
    out.push(native::function(
        &format!("{ns}content_mime"),
        move |args| n.content_mime(args),
    ));
    out.push(native::function(&format!("{ns}mime_types"), |_| {
        Ok(pairs(
            mime::mime_types()
                .into_iter()
                .map(|(k, v)| (k, Tree::Str(v.into()))),
        ))
    }));
    out.push(native::function(&format!("{ns}ext_types"), |_| {
        Ok(pairs(mime::ext_types().into_iter().map(|(k, list)| {
            (
                k,
                Tree::List(list.into_iter().map(|e| Tree::Str(e.into())).collect()),
            )
        })))
    }));
    out.push(native::function(
        &format!("{ns}image_mimes_to_exts"),
        |_| {
            Ok(pairs(
                mime::image_mimes_to_exts()
                    .into_iter()
                    .map(|(k, v)| (k, Tree::Str(v.into()))),
            ))
        },
    ));
    out
}

/// An ordered PHP array from string keys.
fn pairs<'a>(items: impl Iterator<Item = (&'a str, Tree)>) -> Value {
    to_php(&Tree::Map(items.map(|(k, v)| (k.to_owned(), v)).collect()))
}
