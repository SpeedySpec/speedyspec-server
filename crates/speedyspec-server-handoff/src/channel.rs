//! One end of a socketpair carrying JSON lines, with fds riding on the `sendmsg` of their message.

use std::io::{self, IoSlice, IoSliceMut};
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;

use rustix::io::{FdFlags, fcntl_setfd};
use rustix::net::{
    RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
    SendAncillaryMessage, SendFlags, recvmsg, sendmsg,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

/// Most fds one message may carry.
const MAX_FDS: usize = 32;

/// Buffered bytes and fds received so far.
#[derive(Default)]
struct Framer {
    buf: Vec<u8>,
    fds: Vec<OwnedFd>,
}

impl Framer {
    /// The next complete line, with every fd received before its end.
    fn next_line(&mut self) -> Option<(String, Vec<OwnedFd>)> {
        let end = self.buf.iter().position(|b| *b == b'\n')?;
        let line: Vec<u8> = self.buf.drain(..=end).collect();
        Some((
            String::from_utf8_lossy(&line[..end]).into_owned(),
            std::mem::take(&mut self.fds),
        ))
    }

    /// Parses lines until one decodes as `T`, skipping (and logging) messages this build does not know.
    fn next<T: DeserializeOwned>(&mut self) -> Option<(T, Vec<OwnedFd>)> {
        while let Some((line, fds)) = self.next_line() {
            match serde_json::from_str::<T>(&line) {
                Ok(message) => return Some((message, fds)),
                Err(err) => {
                    tracing::warn!(event = "unknown_handoff_message", line = %line, error = %err)
                }
            }
        }
        None
    }
}

fn encode(message: &impl Serialize) -> Vec<u8> {
    let mut line = serde_json::to_vec(message).expect("messages serialize");
    line.push(b'\n');
    line
}

/// Writes all of `bytes`, with `fds` on the first `sendmsg`.
fn send_all(socket: BorrowedFd<'_>, bytes: &[u8], fds: &[BorrowedFd<'_>]) -> io::Result<usize> {
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(MAX_FDS))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    if !fds.is_empty() && !control.push(SendAncillaryMessage::ScmRights(fds)) {
        return Err(io::Error::other(format!(
            "at most {MAX_FDS} fds per message"
        )));
    }
    Ok(sendmsg(
        socket,
        &[IoSlice::new(bytes)],
        &mut control,
        SendFlags::empty(),
    )?)
}

/// One `recvmsg` into the framer. Returns the bytes read (0 at EOF). Received fds are made close-on-exec.
fn recv_once(socket: BorrowedFd<'_>, framer: &mut Framer) -> io::Result<usize> {
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(MAX_FDS))];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    let mut buf = [0u8; 8192];
    let read = recvmsg(
        socket,
        &mut [IoSliceMut::new(&mut buf)],
        &mut control,
        RecvFlags::empty(),
    )?
    .bytes;
    for message in control.drain() {
        if let RecvAncillaryMessage::ScmRights(fds) = message {
            for fd in fds {
                fcntl_setfd(&fd, FdFlags::CLOEXEC)?;
                framer.fds.push(fd);
            }
        }
    }
    framer.buf.extend_from_slice(&buf[..read]);
    Ok(read)
}

/// A blocking channel end.
pub struct Channel {
    stream: UnixStream,
    framer: std::sync::Mutex<Framer>,
}

impl Channel {
    /// A connected pair: this process's end, and the end to hand to a child (placed on fd 3 by the keeper). Both
    /// are close-on-exec.
    pub fn pair() -> io::Result<(Channel, OwnedFd)> {
        let (ours, theirs) = UnixStream::pair()?;
        Ok((Channel::new(ours), OwnedFd::from(theirs)))
    }

    pub fn from_fd(fd: OwnedFd) -> io::Result<Channel> {
        Ok(Channel::new(UnixStream::from(fd)))
    }

    fn new(stream: UnixStream) -> Channel {
        Channel {
            stream,
            framer: std::sync::Mutex::new(Framer::default()),
        }
    }

    pub fn send(&self, message: &impl Serialize, fds: &[BorrowedFd<'_>]) -> io::Result<()> {
        let bytes = encode(message);
        let mut sent = send_all(self.stream.as_fd(), &bytes, fds)?;
        while sent < bytes.len() {
            sent += send_all(self.stream.as_fd(), &bytes[sent..], &[])?;
        }
        Ok(())
    }

    /// Sends a raw line, for tests of unknown messages.
    #[doc(hidden)]
    pub fn send_raw_line(&self, line: &str) -> io::Result<()> {
        send_all(self.stream.as_fd(), format!("{line}\n").as_bytes(), &[]).map(|_| ())
    }

    /// The next message this build knows, with its fds; `None` at EOF.
    pub fn recv<T: DeserializeOwned>(&self) -> io::Result<Option<(T, Vec<OwnedFd>)>> {
        let mut framer = self
            .framer
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        loop {
            if let Some(found) = framer.next() {
                return Ok(Some(found));
            }
            if recv_once(self.stream.as_fd(), &mut framer)? == 0 {
                return Ok(None);
            }
        }
    }

    /// The same end, driven by tokio.
    pub fn into_async(self) -> io::Result<AsyncChannel> {
        self.stream.set_nonblocking(true)?;
        let framer = self
            .framer
            .into_inner()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        Ok(AsyncChannel {
            stream: tokio::net::UnixStream::from_std(self.stream)?,
            framer: tokio::sync::Mutex::new(framer),
            send: tokio::sync::Mutex::new(()),
        })
    }
}

impl AsFd for Channel {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.stream.as_fd()
    }
}

/// A channel end driven by tokio. Sends and receives may run on different tasks at once.
pub struct AsyncChannel {
    stream: tokio::net::UnixStream,
    framer: tokio::sync::Mutex<Framer>,
    send: tokio::sync::Mutex<()>,
}

impl AsyncChannel {
    pub async fn send(&self, message: &impl Serialize, fds: &[BorrowedFd<'_>]) -> io::Result<()> {
        let _one_at_a_time = self.send.lock().await;
        let bytes = encode(message);
        let mut sent = 0;
        let mut pending_fds = fds;
        while sent < bytes.len() {
            self.stream.writable().await?;
            match self.stream.try_io(tokio::io::Interest::WRITABLE, || {
                send_all(self.stream.as_fd(), &bytes[sent..], pending_fds)
            }) {
                Ok(n) => {
                    sent += n;
                    pending_fds = &[];
                }
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                Err(err) => return Err(err),
            }
        }
        Ok(())
    }

    pub async fn recv<T: DeserializeOwned>(&self) -> io::Result<Option<(T, Vec<OwnedFd>)>> {
        let mut framer = self.framer.lock().await;
        loop {
            if let Some(found) = framer.next() {
                return Ok(Some(found));
            }
            self.stream.readable().await?;
            match self.stream.try_io(tokio::io::Interest::READABLE, || {
                recv_once(self.stream.as_fd(), &mut framer)
            }) {
                Ok(0) => return Ok(None),
                Ok(_) => {}
                Err(err) if err.kind() == io::ErrorKind::WouldBlock => {}
                Err(err) => return Err(err),
            }
        }
    }
}
