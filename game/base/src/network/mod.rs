pub mod client;
pub mod server;
pub mod events;
pub mod packet;
pub mod reliable;
pub mod usermessage;

use std::io::{Read, Write};
use std::net::UdpSocket;
use std::os::fd::AsRawFd;
use std::os::unix::net::UnixStream;
use std::sync::Mutex;

pub use client::NetworkClient;
pub use server::NetworkServer;

pub struct NetWake {
    writer: Mutex<UnixStream>,
}

impl NetWake {
    pub fn new(writer: UnixStream) -> Self {
        Self { writer: Mutex::new(writer) }
    }

    pub fn poke(&self) {
        let mut writer = self.writer.lock().unwrap();
        let _ = writer.write(&[1u8]);
    }
}

pub fn wait_socket(socket: &UdpSocket, wake: &mut UnixStream) {
    let mut fds = [
        libc::pollfd { fd: socket.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: wake.as_raw_fd(), events: libc::POLLIN, revents: 0 },
    ];
    let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 2) };
    if ready <= 0 {
        return;
    }

    if fds[1].revents & libc::POLLIN != 0 {
        let mut buf = [0u8; 64];
        loop {
            match wake.read(&mut buf) {
                Ok(0) => {
                    break;
                }
                Ok(_) => {}
                Err(_) => {
                    break;
                }
            }
        }
    }
}
pub use events::{ClientToServer, ServerToClient, NetSend, FromClient, FromServer};
pub use packet::PacketType;
pub use reliable::{EnqueueStatus, ReliableChannel, ReliableBody, SelectiveAck, UnreliableAssembly, UnreliableInbox, accept_unreliable, take_unreliable};

pub const OUTBOUND_CAP: usize = 1024;
pub const RECV_BUDGET: usize = 64;