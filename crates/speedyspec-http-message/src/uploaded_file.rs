use std::cell::RefCell;
use std::rc::Rc;

use speedyspec_http::{Body, HttpError, SharedBody, UploadedFile};

/// A file uploaded with a request.
#[derive(Clone)]
pub struct HttpUploadedFile {
    stream: SharedBody,
    size: Option<u64>,
    error: i32,
    client_filename: Option<String>,
    client_media_type: Option<String>,
    moved: bool,
}

impl HttpUploadedFile {
    pub fn new(
        stream: Box<dyn Body>,
        size: Option<u64>,
        error: i32,
        client_filename: Option<&str>,
        client_media_type: Option<&str>,
    ) -> HttpUploadedFile {
        HttpUploadedFile {
            stream: Rc::new(RefCell::new(stream)),
            size,
            error,
            client_filename: client_filename.map(str::to_string),
            client_media_type: client_media_type.map(str::to_string),
            moved: false,
        }
    }
}

impl UploadedFile for HttpUploadedFile {
    fn size(&self) -> Option<u64> {
        self.size
    }

    fn error(&self) -> i32 {
        self.error
    }

    fn client_filename(&self) -> Option<&str> {
        self.client_filename.as_deref()
    }

    fn client_media_type(&self) -> Option<&str> {
        self.client_media_type.as_deref()
    }

    fn stream(&self) -> Result<SharedBody, HttpError> {
        if self.moved || self.error != 0 {
            return Err(HttpError::Io("the uploaded file is not available".into()));
        }
        Ok(self.stream.clone())
    }

    fn move_to(&mut self, target: &std::path::Path) -> Result<(), HttpError> {
        let bytes = self.stream()?.borrow_mut().read_all()?;
        std::fs::write(target, bytes).map_err(|err| HttpError::Io(err.to_string()))?;
        self.moved = true;
        Ok(())
    }
}
