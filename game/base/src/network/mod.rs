pub mod client;
pub mod server;
pub mod events;
pub mod packet;
pub mod reliable;
pub mod usermessage;

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::Mutex;

pub use client::NetworkClient;
pub use server::NetworkServer;

pub struct NetWake {
    writer: Mutex<TcpStream>,
}

impl NetWake {
    pub fn new(writer: TcpStream) -> Self {
        Self { writer: Mutex::new(writer) }
    }

    pub fn poke(&self) {
        let mut writer = self.writer.lock().unwrap();
        let _ = writer.write(&[1u8]);
    }
}

pub fn wake_pair() -> (TcpStream, TcpStream) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind wake");
    let addr = listener.local_addr().expect("Failed to read wake port");
    let writer = TcpStream::connect(addr).expect("Failed to connect wake");
    let (reader, _) = listener.accept().expect("Failed to accept wake");
    reader.set_nonblocking(true).expect("Failed to set wake nonblocking");
    writer.set_nonblocking(true).expect("Failed to set wake nonblocking");
    reader.set_nodelay(true).expect("Failed to set wake nodelay");
    writer.set_nodelay(true).expect("Failed to set wake nodelay");

    (reader, writer)
}

pub fn wait_socket(socket: &UdpSocket, wake: &mut TcpStream) {
    if !wait_ready(socket, wake) {
        return;
    }

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

#[cfg(unix)]
fn wait_ready(socket: &UdpSocket, wake: &TcpStream) -> bool {
    use std::os::fd::AsRawFd;

    let mut fds = [
        libc::pollfd { fd: socket.as_raw_fd(), events: libc::POLLIN, revents: 0 },
        libc::pollfd { fd: wake.as_raw_fd(), events: libc::POLLIN, revents: 0 },
    ];
    let ready = unsafe { libc::poll(fds.as_mut_ptr(), fds.len() as libc::nfds_t, 2) };

    ready > 0
}

#[cfg(windows)]
fn wait_ready(socket: &UdpSocket, wake: &TcpStream) -> bool {
    use std::os::windows::io::AsRawSocket;

    const POLLIN: i16 = 0x0300;

    #[repr(C)]
    struct PollFd {
        fd: usize,
        events: i16,
        revents: i16,
    }

    #[link(name = "ws2_32")]
    extern "system" {
        fn WSAPoll(fds: *mut PollFd, count: u32, timeout: i32) -> i32;
    }

    let mut fds = [
        PollFd { fd: socket.as_raw_socket() as usize, events: POLLIN, revents: 0 },
        PollFd { fd: wake.as_raw_socket() as usize, events: POLLIN, revents: 0 },
    ];
    let ready = unsafe { WSAPoll(fds.as_mut_ptr(), fds.len() as u32, 2) };

    ready > 0
}
pub use events::{ClientToServer, ServerToClient, NetSend, FromClient, FromServer};
pub use packet::PacketType;
pub use reliable::{EnqueueStatus, ReliableChannel, ReliableBody, SelectiveAck, UnreliableAssembly, UnreliableInbox, accept_unreliable, take_unreliable};

pub const OUTBOUND_CAP: usize = 1024;
pub const RECV_BUDGET: usize = 64;