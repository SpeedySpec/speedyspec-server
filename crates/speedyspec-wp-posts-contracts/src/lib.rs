//! WordPress's post contracts: the post entity and its values, the post type and post status names and resolved
//! values, the errors and the ports.

#[cfg(feature = "conformance")]
pub mod conformance;
mod entity;
mod error;
mod ports;
mod values;

pub use entity::Post;
pub use error::{PostError, PostTypeError};
pub use ports::{
    BadSlugs, Clock, PostReader, PostRepository, PostStatuses, PostTypeSupports, PostTypes,
    RewriteContext, SlugTruncation, TitleSanitizer, Translator,
};
pub use values::{
    CommentStatus, MimeType, PingStatus, PostDate, PostDateTime, PostId, PostStatusName,
    PostStatusObject, PostType, PostTypeName, Slug,
};
