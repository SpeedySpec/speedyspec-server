use std::io::Read;

use speedyspec_http::{Body, HttpError};

/// A body held in memory.
#[derive(Debug, Clone, Default)]
pub struct BytesBody(Vec<u8>);

impl BytesBody {
    pub fn new(bytes: Vec<u8>) -> BytesBody {
        BytesBody(bytes)
    }
}

impl Body for BytesBody {
    fn size(&self) -> Option<u64> {
        Some(self.0.len() as u64)
    }

    fn read_all(&mut self) -> Result<Vec<u8>, HttpError> {
        Ok(self.0.clone())
    }

    fn is_sent(&self) -> bool {
        false
    }
}

/// What PHP gets of a request body after the steps before it ran.
pub enum Replay {
    /// Nothing read it: PHP reads the original source.
    Unread(Box<dyn Read>),
    /// A step read it: PHP gets these exact bytes.
    Buffered(Vec<u8>),
}

/// A request body that costs nothing until a step reads it, and that PHP still receives afterwards.
///
/// A step before PHP may need the body (a firewall rule over it); PHP may need it too. So reading buffers the bytes
/// and [`LazyBody::take_for_php`] hands either the untouched source or the buffer to PHP.
pub struct LazyBody {
    source: Option<Box<dyn Read>>,
    buffer: Option<Vec<u8>>,
    limit: u64,
}

impl LazyBody {
    /// `limit` is the most bytes a step may read; a longer body is an error rather than unbounded memory.
    pub fn new(source: Box<dyn Read>, limit: u64) -> LazyBody {
        LazyBody {
            source: Some(source),
            buffer: None,
            limit,
        }
    }

    pub fn take_for_php(self) -> Replay {
        match (self.buffer, self.source) {
            (Some(bytes), _) => Replay::Buffered(bytes),
            (None, Some(source)) => Replay::Unread(source),
            (None, None) => Replay::Buffered(Vec::new()),
        }
    }
}

impl Body for LazyBody {
    fn size(&self) -> Option<u64> {
        self.buffer.as_ref().map(|bytes| bytes.len() as u64)
    }

    fn read_all(&mut self) -> Result<Vec<u8>, HttpError> {
        if let Some(bytes) = &self.buffer {
            return Ok(bytes.clone());
        }
        let Some(source) = self.source.take() else {
            return Ok(Vec::new());
        };
        let mut bytes = Vec::new();
        source
            .take(self.limit + 1)
            .read_to_end(&mut bytes)
            .map_err(|err| HttpError::Io(err.to_string()))?;
        if bytes.len() as u64 > self.limit {
            return Err(HttpError::Io(format!(
                "request body exceeds {} bytes",
                self.limit
            )));
        }
        self.buffer = Some(bytes.clone());
        Ok(bytes)
    }

    fn is_sent(&self) -> bool {
        false
    }
}

/// The body of a response PHP already wrote to the client.
#[derive(Debug, Clone, Copy, Default)]
pub struct SentBody;

impl Body for SentBody {
    fn size(&self) -> Option<u64> {
        None
    }

    fn read_all(&mut self) -> Result<Vec<u8>, HttpError> {
        Err(HttpError::BodyAlreadySent)
    }

    fn is_sent(&self) -> bool {
        true
    }
}
