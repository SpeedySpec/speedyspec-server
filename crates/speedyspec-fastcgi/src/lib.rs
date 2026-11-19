//! FastCGI 1.0 (<https://fastcgi-archives.github.io/FastCGI_Specification.html>), as the server and its PHP workers
//! speak it.
//!
//! - [`Record`] and the name-value pairs ([`encode_pairs`], [`decode_pairs`]) are the wire format.
//! - [`Responder`] is the worker's side: blocking, one request at a time, answering `GET_VALUES` itself.
//! - [`Client`] is the server's side: async. The request body is sent as `STDIN` as it is pulled from its stream,
//!   and the response is read only as [`Response::next`] is called, so a slow peer on either end holds the other
//!   back instead of being buffered.

use std::io::{self, Read, Write};
use std::path::Path;

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub const VERSION: u8 = 1;
/// The largest content one record carries.
pub const MAX_CONTENT: usize = 65_535;
/// `FCGI_RESPONDER`.
pub const ROLE_RESPONDER: u16 = 1;

/// A record's type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecordType {
    BeginRequest,
    AbortRequest,
    EndRequest,
    Params,
    Stdin,
    Stdout,
    Stderr,
    Data,
    GetValues,
    GetValuesResult,
    UnknownType,
    /// A type this implementation does not know.
    Other(u8),
}

impl RecordType {
    pub fn code(self) -> u8 {
        match self {
            RecordType::BeginRequest => 1,
            RecordType::AbortRequest => 2,
            RecordType::EndRequest => 3,
            RecordType::Params => 4,
            RecordType::Stdin => 5,
            RecordType::Stdout => 6,
            RecordType::Stderr => 7,
            RecordType::Data => 8,
            RecordType::GetValues => 9,
            RecordType::GetValuesResult => 10,
            RecordType::UnknownType => 11,
            RecordType::Other(code) => code,
        }
    }

    pub fn from_code(code: u8) -> RecordType {
        match code {
            1 => RecordType::BeginRequest,
            2 => RecordType::AbortRequest,
            3 => RecordType::EndRequest,
            4 => RecordType::Params,
            5 => RecordType::Stdin,
            6 => RecordType::Stdout,
            7 => RecordType::Stderr,
            8 => RecordType::Data,
            9 => RecordType::GetValues,
            10 => RecordType::GetValuesResult,
            11 => RecordType::UnknownType,
            other => RecordType::Other(other),
        }
    }
}

/// One record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Record {
    pub kind: RecordType,
    pub request_id: u16,
    pub content: Vec<u8>,
}

fn invalid(why: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, why.into())
}

impl Record {
    /// `content` must be at most [`MAX_CONTENT`] bytes.
    pub fn new(kind: RecordType, request_id: u16, content: Vec<u8>) -> Record {
        assert!(
            content.len() <= MAX_CONTENT,
            "a FastCGI record holds at most {MAX_CONTENT} bytes"
        );
        Record {
            kind,
            request_id,
            content,
        }
    }

    /// The record on the wire, padded to a multiple of 8 bytes.
    pub fn encode(&self) -> Vec<u8> {
        let len = self.content.len();
        let padding = (8 - len % 8) % 8;
        let mut out = Vec::with_capacity(8 + len + padding);
        out.push(VERSION);
        out.push(self.kind.code());
        out.extend_from_slice(&self.request_id.to_be_bytes());
        out.extend_from_slice(&(len as u16).to_be_bytes());
        out.push(padding as u8);
        out.push(0);
        out.extend_from_slice(&self.content);
        out.resize(out.len() + padding, 0);
        out
    }

    fn parse_header(header: &[u8; 8]) -> io::Result<(RecordType, u16, usize, usize)> {
        if header[0] != VERSION {
            return Err(invalid(format!("FastCGI version {} is not 1", header[0])));
        }
        let kind = RecordType::from_code(header[1]);
        let request_id = u16::from_be_bytes([header[2], header[3]]);
        let len = u16::from_be_bytes([header[4], header[5]]) as usize;
        Ok((kind, request_id, len, header[6] as usize))
    }

    /// The next record; `None` at a clean end of stream (before a header starts).
    pub fn read_from<R: Read>(reader: &mut R) -> io::Result<Option<Record>> {
        let mut header = [0u8; 8];
        let mut read = 0;
        while read < 8 {
            match reader.read(&mut header[read..]) {
                Ok(0) if read == 0 => return Ok(None),
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "a truncated FastCGI header",
                    ));
                }
                Ok(n) => read += n,
                Err(err) if err.kind() == io::ErrorKind::Interrupted => {}
                Err(err) => return Err(err),
            }
        }
        let (kind, request_id, len, padding) = Self::parse_header(&header)?;
        let mut content = vec![0u8; len + padding];
        reader.read_exact(&mut content)?;
        content.truncate(len);
        Ok(Some(Record {
            kind,
            request_id,
            content,
        }))
    }

    /// As [`Record::read_from`], async.
    pub async fn read_async<R: tokio::io::AsyncRead + Unpin>(
        reader: &mut R,
    ) -> io::Result<Option<Record>> {
        let mut header = [0u8; 8];
        let mut read = 0;
        while read < 8 {
            let n = reader.read(&mut header[read..]).await?;
            if n == 0 {
                if read == 0 {
                    return Ok(None);
                }
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "a truncated FastCGI header",
                ));
            }
            read += n;
        }
        let (kind, request_id, len, padding) = Self::parse_header(&header)?;
        let mut content = vec![0u8; len + padding];
        reader.read_exact(&mut content).await?;
        content.truncate(len);
        Ok(Some(Record {
            kind,
            request_id,
            content,
        }))
    }
}

/// `FCGI_BeginRequestBody`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BeginRequest {
    pub role: u16,
    pub keep_conn: bool,
}

impl BeginRequest {
    pub fn encode(&self) -> [u8; 8] {
        let role = self.role.to_be_bytes();
        [role[0], role[1], u8::from(self.keep_conn), 0, 0, 0, 0, 0]
    }

    pub fn decode(content: &[u8]) -> io::Result<BeginRequest> {
        if content.len() < 8 {
            return Err(invalid("a short FCGI_BEGIN_REQUEST body"));
        }
        Ok(BeginRequest {
            role: u16::from_be_bytes([content[0], content[1]]),
            keep_conn: content[2] & 1 == 1,
        })
    }
}

/// `FCGI_EndRequestBody`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EndRequest {
    pub app_status: u32,
    /// 0 is `FCGI_REQUEST_COMPLETE`.
    pub protocol_status: u8,
}

impl EndRequest {
    pub fn encode(&self) -> [u8; 8] {
        let status = self.app_status.to_be_bytes();
        [
            status[0],
            status[1],
            status[2],
            status[3],
            self.protocol_status,
            0,
            0,
            0,
        ]
    }

    pub fn decode(content: &[u8]) -> io::Result<EndRequest> {
        if content.len() < 8 {
            return Err(invalid("a short FCGI_END_REQUEST body"));
        }
        Ok(EndRequest {
            app_status: u32::from_be_bytes([content[0], content[1], content[2], content[3]]),
            protocol_status: content[4],
        })
    }
}

fn push_length(out: &mut Vec<u8>, len: usize) {
    if len < 128 {
        out.push(len as u8);
    } else {
        out.extend_from_slice(&((len as u32) | 0x8000_0000).to_be_bytes());
    }
}

/// Name-value pairs, as `PARAMS` and `GET_VALUES` carry them.
pub fn encode_pairs<N: AsRef<[u8]>, V: AsRef<[u8]>>(pairs: &[(N, V)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (name, value) in pairs {
        let (name, value) = (name.as_ref(), value.as_ref());
        push_length(&mut out, name.len());
        push_length(&mut out, value.len());
        out.extend_from_slice(name);
        out.extend_from_slice(value);
    }
    out
}

pub fn decode_pairs(mut bytes: &[u8]) -> io::Result<Vec<(Vec<u8>, Vec<u8>)>> {
    fn length(bytes: &mut &[u8]) -> io::Result<usize> {
        let first = *bytes
            .first()
            .ok_or_else(|| invalid("a truncated name-value pair"))?;
        if first & 0x80 == 0 {
            *bytes = &bytes[1..];
            return Ok(first as usize);
        }
        if bytes.len() < 4 {
            return Err(invalid("a truncated name-value pair"));
        }
        let len = u32::from_be_bytes([first & 0x7f, bytes[1], bytes[2], bytes[3]]) as usize;
        *bytes = &bytes[4..];
        Ok(len)
    }
    let mut pairs = Vec::new();
    while !bytes.is_empty() {
        let name_len = length(&mut bytes)?;
        let value_len = length(&mut bytes)?;
        if bytes.len() < name_len + value_len {
            return Err(invalid("a truncated name-value pair"));
        }
        let name = bytes[..name_len].to_vec();
        let value = bytes[name_len..name_len + value_len].to_vec();
        bytes = &bytes[name_len + value_len..];
        pairs.push((name, value));
    }
    Ok(pairs)
}

/// Splits `data` into records of `kind` (an empty `data` writes nothing).
fn records(kind: RecordType, request_id: u16, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for chunk in data.chunks(MAX_CONTENT) {
        out.extend(Record::new(kind, request_id, chunk.to_vec()).encode());
    }
    out
}

// ----- the worker's side

/// A request's head: its id and parameters.
#[derive(Clone, Debug)]
pub struct RequestHead {
    pub id: u16,
    pub params: Vec<(Vec<u8>, Vec<u8>)>,
    pub keep_conn: bool,
}

/// The worker's side of one connection.
pub struct Responder<S: Read + Write> {
    stream: S,
    request: Option<u16>,
    stdin_done: bool,
    pending: Vec<u8>,
    /// What this worker answers to `GET_VALUES`.
    pub values: Vec<(String, String)>,
}

impl<S: Read + Write> Responder<S> {
    pub fn new(stream: S) -> Responder<S> {
        Responder {
            stream,
            request: None,
            stdin_done: false,
            pending: Vec::new(),
            values: vec![
                ("FCGI_MAX_CONNS".into(), "1".into()),
                ("FCGI_MAX_REQS".into(), "1".into()),
                ("FCGI_MPXS_CONNS".into(), "0".into()),
            ],
        }
    }

    /// Answers a management record (request id 0).
    fn management(&mut self, record: &Record) -> io::Result<()> {
        match record.kind {
            RecordType::GetValues => {
                let asked = decode_pairs(&record.content)?;
                let answer: Vec<(Vec<u8>, Vec<u8>)> = asked
                    .into_iter()
                    .filter_map(|(name, _)| {
                        self.values
                            .iter()
                            .find(|(k, _)| k.as_bytes() == name.as_slice())
                            .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
                    })
                    .collect();
                let reply = Record::new(RecordType::GetValuesResult, 0, encode_pairs(&answer));
                self.stream.write_all(&reply.encode())?;
            }
            other => {
                let reply = Record::new(
                    RecordType::UnknownType,
                    0,
                    vec![other.code(), 0, 0, 0, 0, 0, 0, 0],
                );
                self.stream.write_all(&reply.encode())?;
            }
        }
        self.stream.flush()
    }

    /// Waits for the next request (answering `GET_VALUES` meanwhile); `None` when the peer closes.
    pub fn next_request(&mut self) -> io::Result<Option<RequestHead>> {
        let mut head: Option<RequestHead> = None;
        let mut params = Vec::new();
        loop {
            let Some(record) = Record::read_from(&mut self.stream)? else {
                return Ok(None);
            };
            if record.request_id == 0 {
                self.management(&record)?;
                continue;
            }
            match record.kind {
                RecordType::BeginRequest => {
                    let begin = BeginRequest::decode(&record.content)?;
                    head = Some(RequestHead {
                        id: record.request_id,
                        params: Vec::new(),
                        keep_conn: begin.keep_conn,
                    });
                }
                RecordType::Params if head.as_ref().is_some_and(|h| h.id == record.request_id) => {
                    if record.content.is_empty() {
                        let mut head = head.take().expect("checked");
                        head.params = decode_pairs(&params)?;
                        self.request = Some(head.id);
                        self.stdin_done = false;
                        self.pending.clear();
                        return Ok(Some(head));
                    }
                    params.extend_from_slice(&record.content);
                }
                _ => {}
            }
        }
    }

    /// Reads the request body (`STDIN`); 0 at its end.
    pub fn read_stdin(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        while self.pending.is_empty() && !self.stdin_done {
            let Some(record) = Record::read_from(&mut self.stream)? else {
                self.stdin_done = true;
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "the peer closed during the request body",
                ));
            };
            if record.request_id == 0 {
                self.management(&record)?;
                continue;
            }
            if Some(record.request_id) != self.request {
                continue;
            }
            match record.kind {
                RecordType::Stdin if record.content.is_empty() => self.stdin_done = true,
                RecordType::Stdin => self.pending = record.content,
                RecordType::AbortRequest => {
                    self.stdin_done = true;
                    return Err(io::Error::new(
                        io::ErrorKind::ConnectionAborted,
                        "the request was aborted",
                    ));
                }
                _ => {}
            }
        }
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }

    fn id(&self) -> io::Result<u16> {
        self.request
            .ok_or_else(|| io::Error::other("no FastCGI request is open"))
    }

    /// Writes response bytes (`STDOUT`); blocks while the peer is not reading.
    pub fn write_stdout(&mut self, data: &[u8]) -> io::Result<()> {
        let id = self.id()?;
        self.stream
            .write_all(&records(RecordType::Stdout, id, data))
    }

    pub fn write_stderr(&mut self, data: &[u8]) -> io::Result<()> {
        let id = self.id()?;
        self.stream
            .write_all(&records(RecordType::Stderr, id, data))
    }

    /// Ends the request: the closing empty `STDOUT` and `STDERR`, then `END_REQUEST`.
    pub fn end_request(&mut self, app_status: u32) -> io::Result<()> {
        let id = self.id()?;
        let mut out = Record::new(RecordType::Stdout, id, Vec::new()).encode();
        out.extend(Record::new(RecordType::Stderr, id, Vec::new()).encode());
        out.extend(
            Record::new(
                RecordType::EndRequest,
                id,
                EndRequest {
                    app_status,
                    protocol_status: 0,
                }
                .encode()
                .to_vec(),
            )
            .encode(),
        );
        self.stream.write_all(&out)?;
        self.stream.flush()?;
        self.request = None;
        Ok(())
    }

    pub fn stream(&self) -> &S {
        &self.stream
    }
}

// ----- the server's side

/// What a response yields.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Stdout(Bytes),
    Stderr(Bytes),
    End(EndRequest),
}

/// A response being read, as it arrives.
pub struct Response {
    reader: tokio::io::ReadHalf<tokio::net::UnixStream>,
    writer: tokio::task::JoinHandle<io::Result<()>>,
    ended: bool,
}

impl Response {
    /// The next event; `None` after `End`. A connection closed before `End` is an error.
    pub async fn next(&mut self) -> Option<io::Result<Event>> {
        if self.ended {
            return None;
        }
        loop {
            let record = match Record::read_async(&mut self.reader).await {
                Ok(Some(record)) => record,
                Ok(None) => {
                    self.ended = true;
                    return Some(Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "the worker closed the connection before ending the request",
                    )));
                }
                Err(err) => {
                    self.ended = true;
                    return Some(Err(err));
                }
            };
            if record.request_id != REQUEST_ID {
                continue;
            }
            match record.kind {
                RecordType::Stdout if !record.content.is_empty() => {
                    return Some(Ok(Event::Stdout(record.content.into())));
                }
                RecordType::Stderr if !record.content.is_empty() => {
                    return Some(Ok(Event::Stderr(record.content.into())));
                }
                RecordType::EndRequest => {
                    self.ended = true;
                    return Some(EndRequest::decode(&record.content).map(Event::End));
                }
                _ => {}
            }
        }
    }
}

impl Drop for Response {
    fn drop(&mut self) {
        self.writer.abort();
    }
}

const REQUEST_ID: u16 = 1;

/// The server's side.
pub struct Client;

impl Client {
    /// Connects to a worker's socket and sends one request (one connection per request).
    pub async fn request<B>(
        socket: &Path,
        params: Vec<(Vec<u8>, Vec<u8>)>,
        body: B,
    ) -> io::Result<Response>
    where
        B: Stream<Item = io::Result<Bytes>> + Send + 'static,
    {
        let stream = tokio::net::UnixStream::connect(socket).await?;
        Self::request_on(stream, params, body).await
    }

    /// Sends one request on a connected stream. The body is sent as it is pulled; the response is read as asked.
    pub async fn request_on<B>(
        stream: tokio::net::UnixStream,
        params: Vec<(Vec<u8>, Vec<u8>)>,
        body: B,
    ) -> io::Result<Response>
    where
        B: Stream<Item = io::Result<Bytes>> + Send + 'static,
    {
        let (reader, mut writer) = tokio::io::split(stream);
        let mut head = Record::new(
            RecordType::BeginRequest,
            REQUEST_ID,
            BeginRequest {
                role: ROLE_RESPONDER,
                keep_conn: false,
            }
            .encode()
            .to_vec(),
        )
        .encode();
        head.extend(records(
            RecordType::Params,
            REQUEST_ID,
            &encode_pairs(&params),
        ));
        head.extend(Record::new(RecordType::Params, REQUEST_ID, Vec::new()).encode());
        writer.write_all(&head).await?;
        let writer = tokio::spawn(async move {
            let mut body = Box::pin(body);
            while let Some(chunk) = body.next().await {
                let chunk = chunk?;
                if !chunk.is_empty() {
                    writer
                        .write_all(&records(RecordType::Stdin, REQUEST_ID, &chunk))
                        .await?;
                }
            }
            writer
                .write_all(&Record::new(RecordType::Stdin, REQUEST_ID, Vec::new()).encode())
                .await?;
            writer.flush().await
        });
        Ok(Response {
            reader,
            writer,
            ended: false,
        })
    }

    /// Asks a worker for `names` (`GET_VALUES`), which also shows it is answering.
    pub async fn get_values(socket: &Path, names: &[&str]) -> io::Result<Vec<(String, String)>> {
        let mut stream = tokio::net::UnixStream::connect(socket).await?;
        let pairs: Vec<(&[u8], &[u8])> = names
            .iter()
            .map(|n| (n.as_bytes(), b"".as_slice()))
            .collect();
        stream
            .write_all(&Record::new(RecordType::GetValues, 0, encode_pairs(&pairs)).encode())
            .await?;
        loop {
            let Some(record) = Record::read_async(&mut stream).await? else {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "no FCGI_GET_VALUES_RESULT",
                ));
            };
            if record.kind == RecordType::GetValuesResult {
                return Ok(decode_pairs(&record.content)?
                    .into_iter()
                    .map(|(k, v)| {
                        (
                            String::from_utf8_lossy(&k).into_owned(),
                            String::from_utf8_lossy(&v).into_owned(),
                        )
                    })
                    .collect());
            }
        }
    }
}
