//! WordPress's media functions as native PHP functions (Facade, Adapter, Plugin).
//!
//! [`MediaArea`] declares, at the start of every request (from `media.php`) and each inside a `function_exists`
//! guard:
//! - the audio and video functions: `wp_read_audio_metadata()`, `wp_read_video_metadata()`, `wp_add_id3_tag_data()`
//!   and `wp_get_media_creation_timestamp()`;
//! - the image functions: `wp_getimagesize()`, `wp_get_image_mime()`, `wp_get_webp_info()` and `wp_get_avif_info()`;
//! - the MIME list and filetype checks: `wp_get_mime_types()`, `wp_get_ext_types()`, `get_allowed_mime_types()`,
//!   `wp_check_filetype()`, `wp_ext2type()`, `wp_get_default_extension_for_mime_type()` and
//!   `wp_check_filetype_and_ext()`.
//!
//! They run over natives in `SpeedySpec\WpMedia\Native` that apply WordPress's rules (`speedyspec-wp-media`) over the
//! media reader port; fileinfo is reached through PHP's `finfo_file()`.
//!
//! The reader is a container port: [`MediaArea::bind_defaults`] binds the runtime's reader (the isolated, cached
//! FFmpeg reader) under [`MEDIA_READER`], and [`MediaArea::ports`] lets a PHP object implementing
//! `SpeedySpec\WpMedia\MediaReader` replace it, returning the neutral array; `SpeedySpec\WpMedia\Native\read_media()`
//! keeps the default reachable for a replacement that wraps it. The `wp_read_*_metadata` filters run through the Hook
//! API, skipped without entering PHP when nothing is attached; tag values go through PHP's `wp_kses_post()`.

mod adapters;
mod convert;
mod natives;
mod release;
mod twins;

use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_container::Container;
use speedyspec_container_php::ContainerArea;
use speedyspec_php::NativeArea;
use speedyspec_php::ext_php_rs::builders::FunctionBuilder;
use speedyspec_wp_hooks::HookApi;
use speedyspec_wp_media_contracts::{MEDIA_READER, MediaReader, ProbeLimits};

pub use adapters::{
    FileinfoHandle, Pending, PhpFileinfo, PhpReplacement, PhpTagSanitizer, PhpTimeParser,
};
pub use convert::{from_php, to_php};
pub use release::{PORTED_FUNCTIONS, prepare_release, strip_ported};
pub use twins::MediaReaderTwin;

const FACADE: &str = include_str!("media.php");

/// The native area of WordPress's media metadata functions.
pub struct MediaArea {
    natives: Rc<natives::Natives>,
}

impl MediaArea {
    /// The area, filtering through `hooks`, resolving the reader from `container`, reading within `limits`.
    pub fn new(hooks: Rc<HookApi>, container: Rc<Container>, limits: ProbeLimits) -> MediaArea {
        MediaArea {
            natives: Rc::new(natives::Natives {
                hooks,
                container,
                limits,
                default: RefCell::new(None),
                pending: Pending::default(),
                fileinfo: Default::default(),
            }),
        }
    }

    /// Binds `reader` under [`MEDIA_READER`] for the life of the worker; it stays reachable as `read_media()` when
    /// PHP rebinds the port.
    pub fn bind_defaults(&self, reader: Rc<dyn MediaReader>) {
        *self.natives.default.borrow_mut() = Some(reader.clone());
        self.natives
            .container
            .bind_port::<dyn MediaReader>(MEDIA_READER, move |_, _| Ok(reader.clone()));
    }

    /// `area` with the port registered, so a PHP object bound to it reaches Rust callers as a [`PhpReplacement`].
    pub fn ports(&self, area: ContainerArea) -> ContainerArea {
        let pending = self.natives.pending.clone();
        area.port::<dyn MediaReader>(MEDIA_READER, move |object| {
            Rc::new(PhpReplacement::new(object, pending.clone()))
        })
    }
}

impl NativeArea for MediaArea {
    fn functions(&self) -> Vec<FunctionBuilder<'static>> {
        natives::functions(&self.natives)
    }

    fn request_start(&self) {
        // The last request's finfo handle went with it.
        self.natives.fileinfo.clear();
        if let Err(err) = speedyspec_php::eval("speedyspec://wp-media", FACADE) {
            eprintln!("speedyspec_wordpress: installing WordPress's media functions failed: {err}");
        }
    }
}
