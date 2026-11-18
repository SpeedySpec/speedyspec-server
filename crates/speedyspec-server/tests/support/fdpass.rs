//! Proof-only fd passing over a Unix socketpair (Task 4 builds the real channel).

use std::io::IoSlice;
use std::mem::MaybeUninit;
use std::os::fd::{AsFd, BorrowedFd, OwnedFd};
use std::os::unix::net::UnixStream;

use rustix::net::{
    RecvAncillaryBuffer, RecvAncillaryMessage, RecvFlags, SendAncillaryBuffer,
    SendAncillaryMessage, SendFlags, recvmsg, sendmsg,
};

pub fn send(sock: &UnixStream, fds: &[BorrowedFd<'_>]) {
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(4))];
    let mut control = SendAncillaryBuffer::new(&mut space);
    assert!(control.push(SendAncillaryMessage::ScmRights(fds)));
    sendmsg(
        sock.as_fd(),
        &[IoSlice::new(b"F")],
        &mut control,
        SendFlags::empty(),
    )
    .unwrap();
}

pub fn recv(sock: &UnixStream) -> Vec<OwnedFd> {
    let mut space = [MaybeUninit::uninit(); rustix::cmsg_space!(ScmRights(4))];
    let mut control = RecvAncillaryBuffer::new(&mut space);
    let mut byte = [0u8; 1];
    recvmsg(
        sock.as_fd(),
        &mut [std::io::IoSliceMut::new(&mut byte)],
        &mut control,
        RecvFlags::empty(),
    )
    .unwrap();
    let mut fds = Vec::new();
    for message in control.drain() {
        if let RecvAncillaryMessage::ScmRights(received) = message {
            fds.extend(received);
        }
    }
    fds
}
