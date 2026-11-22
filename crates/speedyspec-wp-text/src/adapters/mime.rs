//! Fixed MIME lists.

use std::rc::Rc;

use speedyspec_wp_text_contracts::{MimePairs, MimeTypes, TextError};

use crate::file_name::MimeTable;

/// MIME lists that never change (no `mime_types` or `upload_mimes` filter).
pub struct FixedMimeTypes {
    pub mimes: Rc<MimePairs>,
    pub allowed: Rc<MimePairs>,
}

impl FixedMimeTypes {
    /// WordPress 7.1.5's defaults, for a user without `unfiltered_html`.
    ///
    /// @return
    ///     The lists.
    pub fn wordpress() -> FixedMimeTypes {
        FixedMimeTypes { mimes: Rc::new(MimeTable::wordpress().0), allowed: Rc::new(MimeTable::wordpress_allowed().0) }
    }
}

impl MimeTypes for FixedMimeTypes {
    fn mime_types(&self) -> Result<Rc<MimePairs>, TextError> {
        Ok(self.mimes.clone())
    }

    fn allowed_mime_types(&self) -> Result<Rc<MimePairs>, TextError> {
        Ok(self.allowed.clone())
    }
}
