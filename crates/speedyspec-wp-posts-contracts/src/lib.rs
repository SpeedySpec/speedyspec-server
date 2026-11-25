//! WordPress's post type and post status contracts: the names, the resolved values, the errors and the ports.

mod error;
mod ports;
mod values;

pub use error::PostTypeError;
pub use ports::{
    PostStatuses, PostTypeSupports, PostTypes, RewriteContext, TitleSanitizer, Translator,
};
pub use values::{PostStatusName, PostStatusObject, PostType, PostTypeName};
