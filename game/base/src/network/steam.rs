use std::net::SocketAddr;

pub fn startup(lobby: Option<u64>, connect: Option<&str>)
{
    #[cfg(all(feature = "steam", any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        live::startup(lobby, connect);

        return;
    }

    #[cfg(not(all(feature = "steam", any(target_os = "linux", target_os = "windows", target_os = "macos"))))]
    {
        let _ = (lobby, connect);
    }
}

pub fn join_generation() -> u64
{
    #[cfg(all(feature = "steam", any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        return live::join_generation();
    }

    #[cfg(not(all(feature = "steam", any(target_os = "linux", target_os = "windows", target_os = "macos"))))]
    {
        0
    }
}

pub fn pop_host() -> Option<(Vec<u8>, SocketAddr)>
{
    #[cfg(all(feature = "steam", any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        return live::pop_host();
    }

    #[cfg(not(all(feature = "steam", any(target_os = "linux", target_os = "windows", target_os = "macos"))))]
    {
        None
    }
}

pub fn send_host(addr: SocketAddr, message: &[u8]) -> bool
{
    #[cfg(all(feature = "steam", any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        return live::send_host(addr, message);
    }

    #[cfg(not(all(feature = "steam", any(target_os = "linux", target_os = "windows", target_os = "macos"))))]
    {
        let _ = (addr, message);

        false
    }
}

pub fn pop_client() -> Option<Vec<u8>>
{
    #[cfg(all(feature = "steam", any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        return live::pop_client();
    }

    #[cfg(not(all(feature = "steam", any(target_os = "linux", target_os = "windows", target_os = "macos"))))]
    {
        None
    }
}

pub fn send_client(message: &[u8]) -> bool
{
    #[cfg(all(feature = "steam", any(target_os = "linux", target_os = "windows", target_os = "macos")))]
    {
        return live::send_client(message);
    }

    #[cfg(not(all(feature = "steam", any(target_os = "linux", target_os = "windows", target_os = "macos"))))]
    {
        let _ = message;

        false
    }
}

#[cfg(all(feature = "steam", any(target_os = "linux", target_os = "windows", target_os = "macos")))]
mod live
{
    use std::collections::{HashMap, VecDeque};
    use std::net::{Ipv6Addr, SocketAddr, SocketAddrV6};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use steamworks::networking_sockets::{ListenSocket, NetConnection};
    use steamworks::networking_types::{ListenSocketEvent, NetworkingConfigEntry, NetworkingConnectionState, NetworkingIdentity, SendFlags};
    use steamworks::{AppId, Client, GameLobbyJoinRequested, GameRichPresenceJoinRequested, LobbyId, SteamId};
    #[cfg(feature = "server")]
    use steamworks::LobbyType;

    const PIPE_CAP: usize = 256;

    static JOIN_GEN: AtomicU64 = AtomicU64::new(0);
    static PENDING: AtomicU64 = AtomicU64::new(0);
    static DIAL_ID: AtomicU64 = AtomicU64::new(0);
    static CLIENT_READY: AtomicBool = AtomicBool::new(false);
    static PIPE: Mutex<Pipe> = Mutex::new(Pipe {
        host_in: VecDeque::new(),
        host_out: VecDeque::new(),
        client_in: VecDeque::new(),
        client_out: VecDeque::new(),
    });

    struct Pipe
    {
        host_in: VecDeque<(Vec<u8>, SocketAddr)>,
        host_out: VecDeque<(u64, Vec<u8>)>,
        client_in: VecDeque<Vec<u8>>,
        client_out: VecDeque<Vec<u8>>,
    }

    pub fn startup(lobby: Option<u64>, connect: Option<&str>)
    {
        let client = match open_client()
        {
            Ok(client) => client,
            Err(err) =>
            {
                println!("[steam] {err}");
                if lobby.is_some() || connect.is_some()
                {
                    println!("[steam] friend join unavailable");
                }

                return;
            }
        };

        if let Some(id) = lobby
        {
            note_lobby(id);
        }

        if let Some(text) = connect
        {
            note_connect(text);
        }

        let spawned = std::thread::Builder::new()
            .name("steam".to_string())
            .spawn(move || run(client));
        if let Err(err) = spawned
        {
            println!("[steam] thread failed: {err}");
        }
    }

    pub fn join_generation() -> u64
    {
        JOIN_GEN.load(Ordering::Acquire)
    }

    pub fn pop_host() -> Option<(Vec<u8>, SocketAddr)>
    {
        PIPE.lock().unwrap().host_in.pop_front()
    }

    pub fn send_host(addr: SocketAddr, message: &[u8]) -> bool
    {
        let Some(id) = steam_of(addr) else
        {
            return false;
        };

        let mut pipe = PIPE.lock().unwrap();
        push_cap(&mut pipe.host_out, (id, message.to_vec()));

        true
    }

    pub fn pop_client() -> Option<Vec<u8>>
    {
        PIPE.lock().unwrap().client_in.pop_front()
    }

    pub fn send_client(message: &[u8]) -> bool
    {
        if !CLIENT_READY.load(Ordering::Acquire)
        {
            return false;
        }

        let mut pipe = PIPE.lock().unwrap();
        push_cap(&mut pipe.client_out, message.to_vec());

        true
    }

    fn open_client() -> Result<Client, String>
    {
        if let Ok(text) = std::env::var("ENGINE_STEAM_APPID")
        {
            let id = text.trim().parse::<u32>().map_err(|_| "ENGINE_STEAM_APPID is not a number".to_string())?;

            return Client::init_app(AppId(id)).map_err(|err| err.to_string());
        }

        Client::init().map_err(|err| err.to_string())
    }

    fn note_lobby(id: u64)
    {
        if id == 0
        {
            return;
        }

        PENDING.store(id, Ordering::Release);
        JOIN_GEN.fetch_add(1, Ordering::Release);
    }

    fn note_connect(text: &str)
    {
        let text = text.trim();
        let id_text = text.strip_prefix("lobby:").unwrap_or(text);
        let Ok(id) = id_text.parse::<u64>() else
        {
            println!("[steam] connect string {text}");

            return;
        };

        note_lobby(id);
    }

    fn run(client: Client)
    {
        println!("[steam] {}", client.user().steam_id().raw());
        client.networking_utils().init_relay_network_access();
        let _ = client.networking_sockets().init_authentication();
        let _lobby_join = client.register_callback(|request: GameLobbyJoinRequested| {
            println!("[steam] lobby invite {}", request.lobby_steam_id.raw());
            note_lobby(request.lobby_steam_id.raw());
        });
        let _rich_join = client.register_callback(|request: GameRichPresenceJoinRequested| {
            println!("[steam] rich presence join {}", request.connect);
            note_connect(&request.connect);
        });

        #[cfg(feature = "server")]
        let listen = if PENDING.load(Ordering::Acquire) == 0
        {
            begin_host(&client)
        }
        else
        {
            None
        };
        #[cfg(not(feature = "server"))]
        let listen: Option<ListenSocket> = None;

        let mut peers: HashMap<u64, NetConnection> = HashMap::new();
        let mut dial: Option<NetConnection> = None;
        let mut seen_gen = 0u64;
        let mut dial_after = Instant::now();

        loop
        {
            client.run_callbacks();
            if let Some(sock) = listen.as_ref()
            {
                while let Some(event) = sock.try_receive_event()
                {
                    match event
                    {
                        ListenSocketEvent::Connecting(request) =>
                        {
                            if let Err(err) = request.accept()
                            {
                                println!("[steam] accept failed: {err}");
                            }
                        }
                        ListenSocketEvent::Connected(event) =>
                        {
                            let Some(id) = event.remote().steam_id() else
                            {
                                continue;
                            };

                            let id = id.raw();
                            peers.insert(id, event.take_connection());
                            println!("[steam] peer {id}");
                        }
                        ListenSocketEvent::Disconnected(event) =>
                        {
                            let Some(id) = event.remote().steam_id() else
                            {
                                continue;
                            };

                            let id = id.raw();
                            peers.remove(&id);
                            println!("[steam] peer left {id}");
                        }
                    }
                }
            }

            let gen = JOIN_GEN.load(Ordering::Acquire);
            if gen != seen_gen
            {
                seen_gen = gen;
                let pending = PENDING.load(Ordering::Acquire);
                if pending != 0
                {
                    dial = None;
                    CLIENT_READY.store(false, Ordering::Release);
                    DIAL_ID.store(0, Ordering::Release);
                    client.friends().clear_rich_presence();
                    let hosted = client.clone();
                    client.matchmaking().join_lobby(LobbyId::from_raw(pending), move |result| {
                        if JOIN_GEN.load(Ordering::Acquire) != gen
                        {
                            return;
                        }

                        match result
                        {
                            Ok(lobby) =>
                            {
                                let host = lobby_host(&hosted, lobby);
                                if host == 0
                                {
                                    println!("[steam] lobby {pending} has no host");

                                    return;
                                }

                                DIAL_ID.store(host, Ordering::Release);
                                println!("[steam] joined lobby {pending}");
                            }
                            Err(()) =>
                            {
                                println!("[steam] lobby join failed");
                            }
                        }
                    });
                }
            }

            let want = DIAL_ID.load(Ordering::Acquire);
            if want != 0 && dial.is_none() && Instant::now() >= dial_after
            {
                let identity = NetworkingIdentity::new_steam_id(SteamId::from_raw(want));
                match client.networking_sockets().connect_p2p(identity, 0, std::iter::empty::<NetworkingConfigEntry>())
                {
                    Ok(conn) =>
                    {
                        dial = Some(conn);
                        println!("[steam] dialing {want}");
                    }
                    Err(_) =>
                    {
                        println!("[steam] dial failed");
                        dial_after = Instant::now() + Duration::from_secs(2);
                    }
                }
            }

            let mut closed = false;
            if let Some(conn) = dial.as_ref()
            {
                while let Some(event) = conn.try_receive_event()
                {
                    if event.new_state == NetworkingConnectionState::Connected
                    {
                        CLIENT_READY.store(true, Ordering::Release);
                        println!("[steam] connected");
                    }

                    if event.new_state == NetworkingConnectionState::ClosedByPeer || event.new_state == NetworkingConnectionState::ProblemDetectedLocally
                    {
                        CLIENT_READY.store(false, Ordering::Release);
                        closed = true;
                        println!("[steam] connection closed");
                    }
                }
            }

            if closed
            {
                dial = None;
                dial_after = Instant::now() + Duration::from_secs(1);
            }

            let (host_out, client_out) = {
                let mut pipe = PIPE.lock().unwrap();
                (
                    pipe.host_out.drain(..).collect::<Vec<_>>(),
                    pipe.client_out.drain(..).collect::<Vec<_>>(),
                )
            };
            for (id, bytes) in host_out
            {
                let Some(conn) = peers.get(&id) else
                {
                    continue;
                };

                let _ = conn.send_message(&bytes, SendFlags::UNRELIABLE_NO_NAGLE);
            }

            if CLIENT_READY.load(Ordering::Acquire)
            {
                if let Some(conn) = dial.as_ref()
                {
                    for bytes in client_out
                    {
                        let _ = conn.send_message(&bytes, SendFlags::UNRELIABLE_NO_NAGLE);
                    }
                }
            }

            let mut host_in = Vec::new();
            for (id, conn) in peers.iter_mut()
            {
                let addr = peer_addr(*id);
                conn.receive_messages_with(|message| {
                    host_in.push((message.data().to_vec(), addr));
                });
            }

            let mut client_in = Vec::new();
            if let Some(conn) = dial.as_mut()
            {
                conn.receive_messages_with(|message| {
                    client_in.push(message.data().to_vec());
                });
            }

            {
                let mut pipe = PIPE.lock().unwrap();
                extend_cap(&mut pipe.host_in, host_in);
                extend_cap(&mut pipe.client_in, client_in);
            }

            std::thread::sleep(Duration::from_millis(1));
        }
    }

    #[cfg(feature = "server")]
    fn begin_host(client: &Client) -> Option<ListenSocket>
    {
        match client.networking_sockets().create_listen_socket_p2p(0, std::iter::empty::<NetworkingConfigEntry>())
        {
            Ok(listen) =>
            {
                let hosted = client.clone();
                client.matchmaking().create_lobby(LobbyType::FriendsOnly, 32, move |result| {
                    match result
                    {
                        Ok(lobby) => publish_lobby(&hosted, lobby),
                        Err(err) => println!("[steam] lobby failed: {err}"),
                    }
                });

                Some(listen)
            }
            Err(_) =>
            {
                println!("[steam] listen failed");

                None
            }
        }
    }

    #[cfg(feature = "server")]
    fn publish_lobby(client: &Client, lobby: LobbyId)
    {
        let id = lobby.raw();
        let steam_id = client.user().steam_id().raw();
        client.matchmaking().set_lobby_data(lobby, "host", &steam_id.to_string());
        let connect = format!("lobby:{id}");
        let friends = client.friends();
        friends.set_rich_presence("connect", Some(&connect));
        friends.set_rich_presence("status", Some("Hosting"));
        println!("[steam] lobby {id}");
    }

    fn lobby_host(client: &Client, lobby: LobbyId) -> u64
    {
        let mut host = client.matchmaking().lobby_owner(lobby).raw();
        if let Some(text) = client.matchmaking().lobby_data(lobby, "host")
        {
            if let Ok(id) = text.parse::<u64>()
            {
                if id != 0
                {
                    host = id;
                }
            }
        }

        host
    }

    fn peer_addr(id: u64) -> SocketAddr
    {
        let ip = Ipv6Addr::new(
            0xfd7a,
            0x57ea,
            0x0001,
            0,
            (id >> 48) as u16,
            (id >> 32) as u16,
            (id >> 16) as u16,
            id as u16,
        );

        SocketAddr::V6(SocketAddrV6::new(ip, 1, 0, 0))
    }

    fn steam_of(addr: SocketAddr) -> Option<u64>
    {
        let SocketAddr::V6(v6) = addr else
        {
            return None;
        };

        if v6.port() != 1
        {
            return None;
        }

        let seg = v6.ip().segments();
        if seg[0] != 0xfd7a || seg[1] != 0x57ea || seg[2] != 0x0001 || seg[3] != 0
        {
            return None;
        }

        let id = ((seg[4] as u64) << 48)
            | ((seg[5] as u64) << 32)
            | ((seg[6] as u64) << 16)
            | (seg[7] as u64);
        if id == 0
        {
            return None;
        }

        Some(id)
    }

    fn push_cap<T>(queue: &mut VecDeque<T>, item: T)
    {
        if queue.len() >= PIPE_CAP
        {
            queue.pop_front();
        }

        queue.push_back(item);
    }

    fn extend_cap<T>(queue: &mut VecDeque<T>, items: impl IntoIterator<Item = T>)
    {
        for item in items
        {
            push_cap(queue, item);
        }
    }
}
