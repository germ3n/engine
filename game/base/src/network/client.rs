use crate::network::crypto::{self, Channel, Role, KEY_LEN};
use crate::network::packet::MAX_DATAGRAM;
use crate::network::PacketType;
use std::net::SocketAddr;
use std::net::UdpSocket;
use std::sync::Mutex;
use std::time::{Duration, Instant};

const HELLO_INTERVAL: Duration = Duration::from_millis(250);
const RECV_SPIN: usize = 32;

struct Link {
    secret: [u8; KEY_LEN],
    public: [u8; KEY_LEN],
    channel: Option<Channel>,
    server_public: Option<[u8; KEY_LEN]>,
    rehello: bool,
    last_hello: Option<Instant>,
}

pub struct NetworkClient {
    peer: SocketAddr,
    socket: UdpSocket,
    recv_buf: Vec<u8>,
    steam: bool,
    link: Mutex<Link>,
}

fn new_link() -> Mutex<Link> {
    let secret = crypto::generate_secret();
    Mutex::new(Link {
        public: crypto::public_key(&secret),
        secret,
        channel: None,
        server_public: None,
        rehello: false,
        last_hello: None,
    })
}

impl NetworkClient {
    pub fn new(addr: SocketAddr) -> Self {
        let socket = UdpSocket::bind(addr).unwrap();
        socket.set_nonblocking(true).unwrap();
        Self {
            peer: addr,
            socket,
            recv_buf: Vec::with_capacity(MAX_DATAGRAM + crypto::OVERHEAD),
            steam: false,
            link: new_link(),
        }
    }

    #[allow(dead_code)]
    pub fn from_socket(socket: UdpSocket) -> Self {
        socket.set_nonblocking(true).unwrap();
        Self {
            peer: socket.local_addr().unwrap(),
            socket: socket,
            recv_buf: Vec::with_capacity(MAX_DATAGRAM + crypto::OVERHEAD),
            steam: false,
            link: new_link(),
        }
    }

    pub fn set_steam(&mut self, steam: bool) {
        self.steam = steam;
    }

    pub fn connect(&mut self, addr: SocketAddr) -> Result<(), String> {
        self.socket.connect(addr).map_err(|e| e.to_string())?;
        self.peer = addr;
        self.send_hello(true);
        Ok(())
    }

    fn send_hello(&self, force: bool) {
        let mut link = self.link.lock().unwrap_or_else(|err| err.into_inner());
        if link.channel.is_some() && !link.rehello {
            return;
        }

        let due = link
            .last_hello
            .map_or(true, |at| at.elapsed() >= HELLO_INTERVAL);
        if !force && !due {
            return;
        }

        link.last_hello = Some(Instant::now());
        let bytes = crypto::hello_packet(crypto::TAG_HELLO, &link.public);
        let _ = self.socket.send(&bytes);
    }

    fn handle_hello_ack(&self, server_public: [u8; KEY_LEN]) {
        let mut link = self.link.lock().unwrap_or_else(|err| err.into_inner());
        if link.channel.is_some() && !link.rehello {
            return;
        }

        if link.server_public == Some(server_public) && link.channel.is_some() {
            link.rehello = false;
            return;
        }

        let Some(channel) = Channel::derive(
            Role::Client,
            &link.secret,
            &server_public,
            &link.public,
            &server_public,
        ) else {
            return;
        };

        link.channel = Some(channel);
        link.server_public = Some(server_public);
        link.rehello = false;
    }

    pub fn send_message(&self, message: &[u8]) -> Result<(), String> {
        let Some(message) = crate::network::sim::enqueue_client(message.to_vec()) else {
            return Ok(());
        };

        self.send_raw(&message)
    }

    pub fn flush_sim(&self) {
        crate::network::sim::flush_client(|bytes| {
            let _ = self.send_raw(bytes);
        });
    }

    fn send_raw(&self, message: &[u8]) -> Result<(), String> {
        if self.steam {
            if !crate::network::steam::send_client(message) {
                return Err("steam is not connected".to_string());
            }

            return Ok(());
        }

        let sealed = {
            let link = self.link.lock().unwrap_or_else(|err| err.into_inner());
            link.channel.as_ref().and_then(|channel| channel.seal(message))
        };
        let Some(sealed) = sealed else {
            self.send_hello(false);
            return Err("encrypted channel not established".to_string());
        };

        self.socket.send(&sealed).map_err(|e| e.to_string())?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn get_address(&self) -> SocketAddr {
        self.peer
    }

    pub fn socket(&self) -> &UdpSocket {
        &self.socket
    }

    pub fn poll_packet(&mut self) -> Option<Result<PacketType, ()>> {
        if self.steam {
            let bytes = crate::network::steam::pop_client()?;

            return Some(wincode::deserialize(&bytes).map_err(|_| ()));
        }

        self.send_hello(false);

        let cap = MAX_DATAGRAM + crypto::OVERHEAD;
        if self.recv_buf.len() < cap {
            self.recv_buf.resize(cap, 0);
        }

        for _ in 0..RECV_SPIN {
            let amt = match self.socket.recv_from(&mut self.recv_buf) {
                Ok((amt, _src)) => amt,
                Err(_) => return None,
            };

            let packet = &self.recv_buf[..amt];
            match packet.first().copied() {
                Some(crypto::TAG_HELLO_ACK) => {
                    if let Some(key) = crypto::parse_hello(packet) {
                        self.handle_hello_ack(key);
                    }
                }
                Some(crypto::TAG_RESET) => {
                    let mut link = self.link.lock().unwrap_or_else(|err| err.into_inner());
                    if link.channel.is_some() {
                        link.rehello = true;
                        link.last_hello = None;
                    }
                }
                Some(crypto::TAG_DATA) => {
                    let plain = {
                        let mut link = self.link.lock().unwrap_or_else(|err| err.into_inner());
                        link.channel.as_mut().and_then(|channel| channel.open(packet))
                    };
                    if let Some(plain) = plain {
                        return Some(wincode::deserialize(&plain).map_err(|_| ()));
                    }
                }
                _ => {}
            }
        }

        None
    }
}
