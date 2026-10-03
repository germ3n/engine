use std::net::SocketAddr;

pub struct Start {
    pub lobby: Option<u64>,
    pub connect: Option<String>,
    pub dedicated: bool,
    pub insecure: bool,
    pub map: String,
}

#[derive(Clone)]
pub struct ClientTicket {
    pub host: u64,
    pub steam_id: u64,
    pub name: String,
    pub ticket: Vec<u8>,
}

pub enum AuthUpdate {
    Accepted { addr: SocketAddr, steam_id: u64 },
    Rejected { addr: SocketAddr },
    Revoked { addr: SocketAddr },
}

pub fn startup(start: Start) {
    #[cfg(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    ))]
    {
        live::startup(start);

        return;
    }

    #[cfg(not(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    )))]
    {
        let _ = start;
    }
}

pub fn host_challenge() -> (bool, u64) {
    #[cfg(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    ))]
    {
        return live::host_challenge();
    }

    #[cfg(not(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    )))]
    {
        (false, 0)
    }
}

pub fn peer_id(addr: SocketAddr) -> Option<u64> {
    let SocketAddr::V6(v6) = addr else {
        return None;
    };

    if v6.port() != 1 {
        return None;
    }

    let seg = v6.ip().segments();
    if seg[0] != 0xfd7a || seg[1] != 0x57ea || seg[2] != 0x0001 || seg[3] != 0 {
        return None;
    }

    let id = ((seg[4] as u64) << 48)
        | ((seg[5] as u64) << 32)
        | ((seg[6] as u64) << 16)
        | (seg[7] as u64);
    if id == 0 {
        return None;
    }

    Some(id)
}

pub fn request_ticket(host: u64) {
    #[cfg(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    ))]
    {
        live::request_ticket(host);

        return;
    }

    #[cfg(not(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    )))]
    {
        let _ = host;
    }
}

pub fn take_ticket() -> Option<Result<ClientTicket, ()>> {
    #[cfg(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    ))]
    {
        return live::take_ticket();
    }

    #[cfg(not(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    )))]
    {
        Some(Err(()))
    }
}

pub fn cancel_ticket() {
    #[cfg(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    ))]
    {
        live::cancel_ticket();
    }
}

pub fn begin_auth(addr: SocketAddr, steam_id: u64, ticket: &[u8]) {
    #[cfg(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    ))]
    {
        live::begin_auth(addr, steam_id, ticket);

        return;
    }

    #[cfg(not(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    )))]
    {
        let _ = (addr, steam_id, ticket);
    }
}

pub fn poll_auth() -> Option<AuthUpdate> {
    #[cfg(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    ))]
    {
        return live::poll_auth();
    }

    #[cfg(not(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    )))]
    {
        None
    }
}

pub fn end_session(addr: SocketAddr) {
    #[cfg(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    ))]
    {
        live::end_session(addr);
    }

    #[cfg(not(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    )))]
    {
        let _ = addr;
    }
}

pub fn join_generation() -> u64 {
    #[cfg(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    ))]
    {
        return live::join_generation();
    }

    #[cfg(not(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    )))]
    {
        0
    }
}

pub fn pop_host() -> Option<(Vec<u8>, SocketAddr)> {
    #[cfg(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    ))]
    {
        return live::pop_host();
    }

    #[cfg(not(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    )))]
    {
        None
    }
}

pub fn send_host(addr: SocketAddr, message: &[u8]) -> bool {
    #[cfg(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    ))]
    {
        return live::send_host(addr, message);
    }

    #[cfg(not(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    )))]
    {
        let _ = (addr, message);

        false
    }
}

pub fn pop_client() -> Option<Vec<u8>> {
    #[cfg(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    ))]
    {
        return live::pop_client();
    }

    #[cfg(not(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    )))]
    {
        None
    }
}

pub fn send_client(message: &[u8]) -> bool {
    #[cfg(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    ))]
    {
        return live::send_client(message);
    }

    #[cfg(not(all(
        feature = "steam",
        any(target_os = "linux", target_os = "windows", target_os = "macos")
    )))]
    {
        let _ = message;

        false
    }
}

#[cfg(all(
    feature = "steam",
    any(target_os = "linux", target_os = "windows", target_os = "macos")
))]
mod live {
    use std::collections::{HashMap, VecDeque};
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV6};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::{Duration, Instant};
    use steamworks::networking_sockets::{ListenSocket, NetConnection};
    use steamworks::networking_types::{
        ListenSocketEvent, NetworkingConfigEntry, NetworkingConnectionState, NetworkingIdentity,
        SendFlags,
    };
    #[cfg(feature = "server")]
    use steamworks::LobbyType;
    use steamworks::{
        AppId, AuthSessionTicketResponse, AuthTicket, Client, GameLobbyJoinRequested,
        GameRichPresenceJoinRequested, LobbyId, Server as GameServer, ServerMode, SteamId,
        SteamServersConnected, ValidateAuthTicketResponse,
    };

    const PIPE_CAP: usize = 256;

    const QUERY_PORT: u16 = 27015;
    const TICKET_CAP: usize = 1024;

    static JOIN_GEN: AtomicU64 = AtomicU64::new(0);
    static PENDING: AtomicU64 = AtomicU64::new(0);
    static DIAL_ID: AtomicU64 = AtomicU64::new(0);
    static CLIENT_READY: AtomicBool = AtomicBool::new(false);
    static USER_UP: AtomicBool = AtomicBool::new(false);
    static WANT_SECURE: AtomicBool = AtomicBool::new(false);
    static LOGGED: AtomicBool = AtomicBool::new(false);
    static HOST_ID: AtomicU64 = AtomicU64::new(0);
    static AUTH: Mutex<Auth> = Mutex::new(Auth {
        ticket_host: 0,
        ticket: TicketPhase::Idle,
        ticket_handle: None,
        ticket_bytes: Vec::new(),
        cancel: false,
        local_id: 0,
        local_name: String::new(),
        jobs: VecDeque::new(),
        updates: VecDeque::new(),
        ending: VecDeque::new(),
        verdicts: VecDeque::new(),
    });
    static PIPE: Mutex<Pipe> = Mutex::new(Pipe {
        host_in: VecDeque::new(),
        host_out: VecDeque::new(),
        client_in: VecDeque::new(),
        client_out: VecDeque::new(),
    });

    struct Pipe {
        host_in: VecDeque<(Vec<u8>, SocketAddr)>,
        host_out: VecDeque<(u64, Vec<u8>)>,
        client_in: VecDeque<Vec<u8>>,
        client_out: VecDeque<Vec<u8>>,
    }

    enum TicketPhase {
        Idle,
        Waiting,
        Ready(Result<super::ClientTicket, ()>),
    }

    struct AuthJob {
        addr: SocketAddr,
        steam_id: u64,
        ticket: Vec<u8>,
    }

    struct Verdict {
        steam_id: u64,
        ok: bool,
    }

    struct Auth {
        ticket_host: u64,
        ticket: TicketPhase,
        ticket_handle: Option<AuthTicket>,
        ticket_bytes: Vec<u8>,
        cancel: bool,
        local_id: u64,
        local_name: String,
        jobs: VecDeque<AuthJob>,
        updates: VecDeque<super::AuthUpdate>,
        ending: VecDeque<SocketAddr>,
        verdicts: VecDeque<Verdict>,
    }

    struct Session {
        addr: SocketAddr,
        accepted: bool,
    }

    trait SteamAuth {
        fn begin_auth(
            &self,
            id: SteamId,
            ticket: &[u8],
        ) -> Result<(), steamworks::AuthSessionError>;
        fn end_auth(&self, id: SteamId);
        fn cancel_auth_ticket(&self, ticket: AuthTicket);
        fn issue_ticket(&self, host: u64) -> Option<(AuthTicket, Vec<u8>)>;
    }

    impl SteamAuth for Client {
        fn begin_auth(
            &self,
            id: SteamId,
            ticket: &[u8],
        ) -> Result<(), steamworks::AuthSessionError> {
            self.user().begin_authentication_session(id, ticket)
        }

        fn end_auth(&self, id: SteamId) {
            self.user().end_authentication_session(id);
        }

        fn cancel_auth_ticket(&self, ticket: AuthTicket) {
            self.user().cancel_authentication_ticket(ticket);
        }

        fn issue_ticket(&self, host: u64) -> Option<(AuthTicket, Vec<u8>)> {
            Some(
                self.user()
                    .authentication_session_ticket_with_steam_id(SteamId::from_raw(host)),
            )
        }
    }

    impl SteamAuth for GameServer {
        fn begin_auth(
            &self,
            id: SteamId,
            ticket: &[u8],
        ) -> Result<(), steamworks::AuthSessionError> {
            self.begin_authentication_session(id, ticket)
        }

        fn end_auth(&self, id: SteamId) {
            self.end_authentication_session(id);
        }

        fn cancel_auth_ticket(&self, ticket: AuthTicket) {
            let _ = ticket;
        }

        fn issue_ticket(&self, _host: u64) -> Option<(AuthTicket, Vec<u8>)> {
            None
        }
    }

    pub fn startup(start: super::Start) {
        if start.dedicated && start.insecure {
            log::info!("[steam] insecure");

            return;
        }

        if let Some(id) = start.lobby {
            note_lobby(id);
        }

        if let Some(text) = start.connect.as_deref() {
            note_connect(text);
        }

        if start.dedicated {
            let server = match open_server(&start.map) {
                Ok(server) => server,
                Err(err) => {
                    log::warn!("[steam] {err}");
                    log::info!("[steam] insecure");

                    return;
                }
            };
            let advertise = log_on_server(&server);
            WANT_SECURE.store(true, Ordering::Release);
            let spawned = std::thread::Builder::new()
                .name("steam".to_string())
                .spawn(move || run_game(server, advertise));
            if let Err(err) = spawned {
                WANT_SECURE.store(false, Ordering::Release);
                log::warn!("[steam] thread failed: {err}");
                log::info!("[steam] insecure");
            }

            return;
        }

        let client = match open_client() {
            Ok(client) => client,
            Err(err) => {
                log::warn!("[steam] {err}");
                if start.lobby.is_some() || start.connect.is_some() {
                    log::warn!("[steam] friend join unavailable");
                }

                log::info!("[steam] insecure");

                return;
            }
        };

        if start.insecure {
            log::info!("[steam] insecure");
        }

        WANT_SECURE.store(!start.insecure, Ordering::Release);
        USER_UP.store(true, Ordering::Release);
        let spawned = std::thread::Builder::new()
            .name("steam".to_string())
            .spawn(move || run(client));
        if let Err(err) = spawned {
            USER_UP.store(false, Ordering::Release);
            WANT_SECURE.store(false, Ordering::Release);
            log::warn!("[steam] thread failed: {err}");
            log::info!("[steam] insecure");
        }
    }

    pub fn host_challenge() -> (bool, u64) {
        if !WANT_SECURE.load(Ordering::Acquire) {
            return (false, 0);
        }

        if !LOGGED.load(Ordering::Acquire) {
            return (true, 0);
        }

        (true, HOST_ID.load(Ordering::Acquire))
    }

    pub fn request_ticket(host: u64) {
        let mut auth = AUTH.lock().unwrap();
        if !USER_UP.load(Ordering::Acquire) {
            auth.ticket_host = host;
            auth.ticket = TicketPhase::Ready(Err(()));

            return;
        }

        if auth.ticket_host == host && !matches!(auth.ticket, TicketPhase::Idle) {
            return;
        }

        auth.cancel = auth.ticket_handle.is_some();
        auth.ticket_host = host;
        auth.ticket_bytes.clear();
        auth.ticket = TicketPhase::Waiting;
    }

    pub fn take_ticket() -> Option<Result<super::ClientTicket, ()>> {
        let auth = AUTH.lock().unwrap();
        match &auth.ticket {
            TicketPhase::Ready(result) => Some(result.clone()),
            _ => None,
        }
    }

    pub fn cancel_ticket() {
        let mut auth = AUTH.lock().unwrap();
        auth.cancel = auth.ticket_handle.is_some();
        auth.ticket_host = 0;
        auth.ticket_bytes.clear();
        auth.ticket = TicketPhase::Idle;
    }

    pub fn begin_auth(addr: SocketAddr, steam_id: u64, ticket: &[u8]) {
        let mut auth = AUTH.lock().unwrap();
        push_cap(
            &mut auth.jobs,
            AuthJob {
                addr,
                steam_id,
                ticket: ticket.to_vec(),
            },
        );
    }

    pub fn poll_auth() -> Option<super::AuthUpdate> {
        AUTH.lock().unwrap().updates.pop_front()
    }

    pub fn end_session(addr: SocketAddr) {
        let mut auth = AUTH.lock().unwrap();
        push_cap(&mut auth.ending, addr);
    }

    pub fn join_generation() -> u64 {
        JOIN_GEN.load(Ordering::Acquire)
    }

    pub fn pop_host() -> Option<(Vec<u8>, SocketAddr)> {
        PIPE.lock().unwrap().host_in.pop_front()
    }

    pub fn send_host(addr: SocketAddr, message: &[u8]) -> bool {
        let Some(id) = steam_of(addr) else {
            return false;
        };

        let mut pipe = PIPE.lock().unwrap();
        push_cap(&mut pipe.host_out, (id, message.to_vec()));

        true
    }

    pub fn pop_client() -> Option<Vec<u8>> {
        PIPE.lock().unwrap().client_in.pop_front()
    }

    pub fn send_client(message: &[u8]) -> bool {
        if !CLIENT_READY.load(Ordering::Acquire) {
            return false;
        }

        let mut pipe = PIPE.lock().unwrap();
        push_cap(&mut pipe.client_out, message.to_vec());

        true
    }

    fn open_client() -> Result<Client, String> {
        if let Ok(text) = std::env::var("ENGINE_STEAM_APPID") {
            let id = text
                .trim()
                .parse::<u32>()
                .map_err(|_| "ENGINE_STEAM_APPID is not a number".to_string())?;

            return Client::init_app(AppId(id)).map_err(init_message);
        }

        Client::init().map_err(init_message)
    }

    fn init_message(err: steamworks::SteamAPIInitError) -> String {
        match err {
            steamworks::SteamAPIInitError::FailedGeneric(text)
            | steamworks::SteamAPIInitError::NoSteamClient(text)
            | steamworks::SteamAPIInitError::VersionMismatch(text) => {
                if text.is_empty() {
                    return "Steam is not running".to_string();
                }

                text
            }
        }
    }

    fn note_lobby(id: u64) {
        if id == 0 {
            return;
        }

        PENDING.store(id, Ordering::Release);
        JOIN_GEN.fetch_add(1, Ordering::Release);
    }

    fn note_connect(text: &str) {
        let text = text.trim();
        let id_text = text.strip_prefix("lobby:").unwrap_or(text);
        let Ok(id) = id_text.parse::<u64>() else {
            log::info!("[steam] connect string {text}");

            return;
        };

        note_lobby(id);
    }

    fn run(client: Client) {
        log::info!("[steam] {}", client.user().steam_id().raw());
        client.networking_utils().init_relay_network_access();
        let _ = client.networking_sockets().init_authentication();
        let _lobby_join = client.register_callback(|request: GameLobbyJoinRequested| {
            log::info!("[steam] lobby invite {}", request.lobby_steam_id.raw());
            note_lobby(request.lobby_steam_id.raw());
        });
        let _rich_join = client.register_callback(|request: GameRichPresenceJoinRequested| {
            log::info!("[steam] rich presence join {}", request.connect);
            note_connect(&request.connect);
        });
        let _ticket_cb = client.register_callback(|response: AuthSessionTicketResponse| {
            note_ticket(response);
        });
        let _validate_cb = client.register_callback(|response: ValidateAuthTicketResponse| {
            note_verdict(response);
        });

        #[cfg(feature = "server")]
        let listen = if PENDING.load(Ordering::Acquire) == 0 {
            begin_host(&client)
        } else {
            None
        };
        #[cfg(not(feature = "server"))]
        let listen: Option<ListenSocket> = None;

        let mut peers: HashMap<u64, NetConnection> = HashMap::new();
        let mut dial: Option<NetConnection> = None;
        let mut seen_gen = 0u64;
        let mut dial_after = Instant::now();
        let mut sessions: HashMap<u64, Session> = HashMap::new();
        let mut by_addr: HashMap<SocketAddr, u64> = HashMap::new();

        loop {
            refresh_user(&client);
            client.run_callbacks();
            pump(&client, &mut sessions, &mut by_addr);
            if let Some(sock) = listen.as_ref() {
                while let Some(event) = sock.try_receive_event() {
                    match event {
                        ListenSocketEvent::Connecting(request) => {
                            if let Err(err) = request.accept() {
                                log::warn!("[steam] accept failed: {err}");
                            }
                        }
                        ListenSocketEvent::Connected(event) => {
                            let Some(id) = event.remote().steam_id() else {
                                continue;
                            };

                            let id = id.raw();
                            peers.insert(id, event.take_connection());
                            log::info!("[steam] peer {id}");
                        }
                        ListenSocketEvent::Disconnected(event) => {
                            let Some(id) = event.remote().steam_id() else {
                                continue;
                            };

                            let id = id.raw();
                            peers.remove(&id);
                            log::info!("[steam] peer left {id}");
                        }
                    }
                }
            }

            let gen = JOIN_GEN.load(Ordering::Acquire);
            if gen != seen_gen {
                seen_gen = gen;
                let pending = PENDING.load(Ordering::Acquire);
                if pending != 0 {
                    dial = None;
                    CLIENT_READY.store(false, Ordering::Release);
                    DIAL_ID.store(0, Ordering::Release);
                    client.friends().clear_rich_presence();
                    let hosted = client.clone();
                    client
                        .matchmaking()
                        .join_lobby(LobbyId::from_raw(pending), move |result| {
                            if JOIN_GEN.load(Ordering::Acquire) != gen {
                                return;
                            }

                            match result {
                                Ok(lobby) => {
                                    let host = lobby_host(&hosted, lobby);
                                    if host == 0 {
                                        log::warn!("[steam] lobby {pending} has no host");

                                        return;
                                    }

                                    DIAL_ID.store(host, Ordering::Release);
                                    log::info!("[steam] joined lobby {pending}");
                                }
                                Err(()) => {
                                    log::warn!("[steam] lobby join failed");
                                }
                            }
                        });
                }
            }

            let want = DIAL_ID.load(Ordering::Acquire);
            if want != 0 && dial.is_none() && Instant::now() >= dial_after {
                let identity = NetworkingIdentity::new_steam_id(SteamId::from_raw(want));
                match client.networking_sockets().connect_p2p(
                    identity,
                    0,
                    std::iter::empty::<NetworkingConfigEntry>(),
                ) {
                    Ok(conn) => {
                        dial = Some(conn);
                        log::info!("[steam] dialing {want}");
                    }
                    Err(_) => {
                        log::warn!("[steam] dial failed");
                        dial_after = Instant::now() + Duration::from_secs(2);
                    }
                }
            }

            let mut closed = false;
            if let Some(conn) = dial.as_ref() {
                while let Some(event) = conn.try_receive_event() {
                    if event.new_state == NetworkingConnectionState::Connected {
                        CLIENT_READY.store(true, Ordering::Release);
                        log::info!("[steam] connected");
                    }

                    if event.new_state == NetworkingConnectionState::ClosedByPeer
                        || event.new_state == NetworkingConnectionState::ProblemDetectedLocally
                    {
                        CLIENT_READY.store(false, Ordering::Release);
                        closed = true;
                        log::info!("[steam] connection closed");
                    }
                }
            }

            if closed {
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
            for (id, bytes) in host_out {
                let Some(conn) = peers.get(&id) else {
                    continue;
                };

                let _ = conn.send_message(&bytes, SendFlags::UNRELIABLE_NO_NAGLE);
            }

            if CLIENT_READY.load(Ordering::Acquire) {
                if let Some(conn) = dial.as_ref() {
                    for bytes in client_out {
                        let _ = conn.send_message(&bytes, SendFlags::UNRELIABLE_NO_NAGLE);
                    }
                }
            }

            let mut host_in = Vec::new();
            for (id, conn) in peers.iter_mut() {
                let addr = peer_addr(*id);
                conn.receive_messages_with(|message| {
                    host_in.push((message.data().to_vec(), addr));
                });
            }

            let mut client_in = Vec::new();
            if let Some(conn) = dial.as_mut() {
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
    fn begin_host(client: &Client) -> Option<ListenSocket> {
        match client
            .networking_sockets()
            .create_listen_socket_p2p(0, std::iter::empty::<NetworkingConfigEntry>())
        {
            Ok(listen) => {
                let hosted = client.clone();
                client
                    .matchmaking()
                    .create_lobby(LobbyType::FriendsOnly, 32, move |result| match result {
                        Ok(lobby) => publish_lobby(&hosted, lobby),
                        Err(err) => log::warn!("[steam] lobby failed: {err}"),
                    });

                Some(listen)
            }
            Err(_) => {
                log::warn!("[steam] listen failed");

                None
            }
        }
    }

    #[cfg(feature = "server")]
    fn publish_lobby(client: &Client, lobby: LobbyId) {
        let id = lobby.raw();
        let steam_id = client.user().steam_id().raw();
        client
            .matchmaking()
            .set_lobby_data(lobby, "host", &steam_id.to_string());
        let connect = format!("lobby:{id}");
        let friends = client.friends();
        friends.set_rich_presence("connect", Some(&connect));
        friends.set_rich_presence("status", Some("Hosting"));
        log::info!("[steam] lobby {id}");
    }

    fn lobby_host(client: &Client, lobby: LobbyId) -> u64 {
        let mut host = client.matchmaking().lobby_owner(lobby).raw();
        if let Some(text) = client.matchmaking().lobby_data(lobby, "host") {
            if let Ok(id) = text.parse::<u64>() {
                if id != 0 {
                    host = id;
                }
            }
        }

        host
    }

    fn peer_addr(id: u64) -> SocketAddr {
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

    fn steam_of(addr: SocketAddr) -> Option<u64> {
        super::peer_id(addr)
    }

    fn open_server(map: &str) -> Result<GameServer, String> {
        let (server, _client) = GameServer::init(
            Ipv4Addr::UNSPECIFIED,
            25400,
            QUERY_PORT,
            ServerMode::Authentication,
            "1.0",
        )
        .map_err(init_message)?;
        let product = server.utils().app_id().0.to_string();
        server.set_product(&product);
        server.set_game_description("engine");
        server.set_dedicated_server(true);
        server.set_max_players(128);
        server.set_server_name("engine");
        let map_name = if map.is_empty() { "hall" } else { map };
        server.set_map_name(map_name);

        Ok(server)
    }

    fn log_on_server(server: &GameServer) -> bool {
        if let Ok(token) = std::env::var("ENGINE_STEAM_GSLT") {
            let token = token.trim();
            if !token.is_empty() {
                server.log_on(token);

                return true;
            }
        }

        server.log_on_anonymous();

        false
    }

    fn run_game(server: GameServer, advertise: bool) {
        let hooked = server.clone();
        let _validate_cb = server.register_callback(|response: ValidateAuthTicketResponse| {
            note_verdict(response);
        });
        let _connected = server.register_callback(move |_: SteamServersConnected| {
            mark_logged(&hooked, advertise);
        });
        let mut sessions: HashMap<u64, Session> = HashMap::new();
        let mut by_addr: HashMap<SocketAddr, u64> = HashMap::new();

        loop {
            server.run_callbacks();
            mark_logged(&server, advertise);
            pump(&server, &mut sessions, &mut by_addr);
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn mark_logged(server: &GameServer, advertise: bool) {
        let id = server.steam_id().raw();
        if id == 0 {
            return;
        }

        HOST_ID.store(id, Ordering::Release);
        if LOGGED.swap(true, Ordering::AcqRel) {
            return;
        }

        log::info!("[steam] secure {id}");
        if advertise {
            server.set_advertise_server_active(true);
        }
    }

    fn refresh_user(client: &Client) {
        let id = client.user().steam_id().raw();
        let logged = client.user().logged_on() && id != 0;
        let name = trim_persona(&client.friends().name());
        {
            let mut auth = AUTH.lock().unwrap();
            auth.local_id = id;
            auth.local_name = name;
        }

        if !WANT_SECURE.load(Ordering::Acquire) {
            return;
        }

        HOST_ID.store(id, Ordering::Release);
        let was = LOGGED.swap(logged, Ordering::AcqRel);
        if logged && !was {
            log::info!("[steam] secure {id}");
        }
    }

    fn note_ticket(response: AuthSessionTicketResponse) {
        let mut auth = AUTH.lock().unwrap();
        if !matches!(auth.ticket, TicketPhase::Waiting) {
            return;
        }

        if auth.ticket_handle != Some(response.ticket) || auth.ticket_bytes.is_empty() {
            return;
        }

        if response.result.is_err() {
            auth.ticket = TicketPhase::Ready(Err(()));

            return;
        }

        auth.ticket = TicketPhase::Ready(Ok(super::ClientTicket {
            host: auth.ticket_host,
            steam_id: auth.local_id,
            name: auth.local_name.clone(),
            ticket: auth.ticket_bytes.clone(),
        }));
    }

    fn note_verdict(response: ValidateAuthTicketResponse) {
        let mut auth = AUTH.lock().unwrap();
        push_cap(
            &mut auth.verdicts,
            Verdict {
                steam_id: response.steam_id.raw(),
                ok: response.response.is_ok(),
            },
        );
    }

    fn pump<T: SteamAuth>(
        api: &T,
        sessions: &mut HashMap<u64, Session>,
        by_addr: &mut HashMap<SocketAddr, u64>,
    ) {
        service_ticket(api);
        let (verdicts, jobs, ending) = {
            let mut auth = AUTH.lock().unwrap();
            (
                auth.verdicts.drain(..).collect::<Vec<_>>(),
                auth.jobs.drain(..).collect::<Vec<_>>(),
                auth.ending.drain(..).collect::<Vec<_>>(),
            )
        };

        for verdict in verdicts {
            let Some(session) = sessions.get(&verdict.steam_id) else {
                continue;
            };

            let addr = session.addr;
            let accepted = session.accepted;
            if verdict.ok {
                if !accepted {
                    if let Some(session) = sessions.get_mut(&verdict.steam_id) {
                        session.accepted = true;
                    }

                    push_update(super::AuthUpdate::Accepted {
                        addr,
                        steam_id: verdict.steam_id,
                    });
                }

                continue;
            }

            sessions.remove(&verdict.steam_id);
            by_addr.remove(&addr);
            api.end_auth(SteamId::from_raw(verdict.steam_id));
            if accepted {
                push_update(super::AuthUpdate::Revoked { addr });
            } else {
                push_update(super::AuthUpdate::Rejected { addr });
            }
        }

        for addr in ending {
            let Some(id) = by_addr.remove(&addr) else {
                continue;
            };

            sessions.remove(&id);
            api.end_auth(SteamId::from_raw(id));
        }

        for job in jobs {
            if let Some(old) = by_addr.remove(&job.addr) {
                sessions.remove(&old);
                api.end_auth(SteamId::from_raw(old));
            }

            if let Some(old) = sessions.remove(&job.steam_id) {
                by_addr.remove(&old.addr);
                api.end_auth(SteamId::from_raw(job.steam_id));
                if old.accepted {
                    push_update(super::AuthUpdate::Revoked { addr: old.addr });
                } else {
                    push_update(super::AuthUpdate::Rejected { addr: old.addr });
                }
            }

            if job.ticket.len() > TICKET_CAP {
                push_update(super::AuthUpdate::Rejected { addr: job.addr });

                continue;
            }

            match api.begin_auth(SteamId::from_raw(job.steam_id), &job.ticket) {
                Ok(()) => {
                    sessions.insert(
                        job.steam_id,
                        Session {
                            addr: job.addr,
                            accepted: false,
                        },
                    );
                    by_addr.insert(job.addr, job.steam_id);
                }
                Err(err) => {
                    log::info!("[steam] auth {err}");
                    push_update(super::AuthUpdate::Rejected { addr: job.addr });
                }
            }
        }
    }

    fn service_ticket<T: SteamAuth>(api: &T) {
        let cancel = {
            let mut auth = AUTH.lock().unwrap();
            if auth.cancel {
                auth.cancel = false;
                auth.ticket_handle.take()
            } else {
                None
            }
        };
        if let Some(handle) = cancel {
            api.cancel_auth_ticket(handle);
        }

        let host = {
            let auth = AUTH.lock().unwrap();
            match auth.ticket {
                TicketPhase::Waiting if auth.ticket_handle.is_none() => Some(auth.ticket_host),
                _ => None,
            }
        };
        let Some(host) = host else {
            return;
        };

        if host == 0 {
            let mut auth = AUTH.lock().unwrap();
            if matches!(auth.ticket, TicketPhase::Waiting) {
                auth.ticket = TicketPhase::Ready(Err(()));
            }

            return;
        }

        let Some((handle, bytes)) = api.issue_ticket(host) else {
            let mut auth = AUTH.lock().unwrap();
            if matches!(auth.ticket, TicketPhase::Waiting) {
                auth.ticket = TicketPhase::Ready(Err(()));
            }

            return;
        };

        let mut auth = AUTH.lock().unwrap();
        if auth.ticket_host != host || !matches!(auth.ticket, TicketPhase::Waiting) {
            drop(auth);
            api.cancel_auth_ticket(handle);

            return;
        }

        if bytes.is_empty() || bytes.len() > TICKET_CAP {
            auth.ticket = TicketPhase::Ready(Err(()));
            drop(auth);
            api.cancel_auth_ticket(handle);

            return;
        }

        auth.ticket_handle = Some(handle);
        auth.ticket_bytes = bytes;
    }

    fn push_update(update: super::AuthUpdate) {
        let mut auth = AUTH.lock().unwrap();
        push_cap(&mut auth.updates, update);
    }

    fn trim_persona(name: &str) -> String {
        name.chars()
            .filter(|ch| !ch.is_control())
            .take(32)
            .collect()
    }

    fn push_cap<T>(queue: &mut VecDeque<T>, item: T) {
        if queue.len() >= PIPE_CAP {
            queue.pop_front();
        }

        queue.push_back(item);
    }

    fn extend_cap<T>(queue: &mut VecDeque<T>, items: impl IntoIterator<Item = T>) {
        for item in items {
            push_cap(queue, item);
        }
    }
}
