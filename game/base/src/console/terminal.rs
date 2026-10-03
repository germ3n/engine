use super::{exec_line, strip_comment, tokenize, ConVar, ConVarValue, AUTOCOMPLETE_KEY};
use crate::demo;
use crate::input::Binds;
use crate::script::Realm;
use std::collections::HashMap;
use std::io::{BufRead, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
pub struct ConsoleSide {
    pub cvars: Arc<HashMap<String, Arc<ConVar>>>,
    pub binds: Arc<Mutex<Binds>>,
}

struct Sides {
    server: Option<ConsoleSide>,
    client: Option<ConsoleSide>,
}

static SIDES: Mutex<Sides> = Mutex::new(Sides {
    server: None,
    client: None,
});

pub fn bind_sides(server: Option<ConsoleSide>, client: Option<ConsoleSide>) {
    let mut sides = SIDES.lock().unwrap();
    sides.server = server;
    sides.client = client;
}

fn snapshot_sides() -> (Option<ConsoleSide>, Option<ConsoleSide>) {
    let sides = SIDES.lock().unwrap();

    (sides.server.clone(), sides.client.clone())
}

pub(crate) fn submit_shared(line: &str) -> Vec<Outcome> {
    let (server, client) = snapshot_sides();

    dispatch(line, server.as_ref(), client.as_ref())
}

pub(crate) fn complete_shared(line: &str, lua: &mlua::Lua, realm: Realm) -> (String, Vec<String>) {
    let (server, client) = snapshot_sides();
    let matches = suggest_with(line, server.as_ref(), client.as_ref(), Some((lua, realm)));
    let mut cycle = WINDOW_CYCLE.lock().unwrap();
    let applied = apply_completion(line, &matches, &mut cycle);

    (applied.line, applied.list)
}

pub(crate) struct Outcome {
    pub side: &'static str,
    pub line: String,
    pub detail: Option<String>,
    pub error: Option<String>,
    pub quit: bool,
}

enum Hit {
    Done(Result<Option<String>, String>),
    Miss,
    Quit,
    Local,
}

pub fn spawn_terminal(server: Option<ConsoleSide>, client: Option<ConsoleSide>) {
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        drop(server);
        drop(client);

        return;
    }

    #[cfg(not(any(target_os = "android", target_os = "ios")))]
    {
        std::thread::spawn(move || read_terminal(server, client));
    }
}

const COMMANDS: &[&str] = &[
    "bind",
    "unbind",
    "unbindall",
    "exec",
    "host_writeconfig",
    "record",
    "stop",
    "playdemo",
    "demo_pause",
    "demo_timescale",
    "demo_seek",
    "demo_loop",
    "demo_cam",
    "demo_view",
    "quit",
    "exit",
];

fn read_terminal(server: Option<ConsoleSide>, client: Option<ConsoleSide>) {
    if enable_raw() {
        let _guard = RawGuard;
        read_keys(server, client);

        return;
    }

    read_cooked(server, client);
}

fn read_cooked(server: Option<ConsoleSide>, client: Option<ConsoleSide>) {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();

    loop {
        print!("] ");
        let _ = std::io::stdout().flush();
        let mut line = String::new();

        match input.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if !submit(&line, server.as_ref(), client.as_ref()) {
                    break;
                }
            }
        }
    }
}

fn read_keys(server: Option<ConsoleSide>, client: Option<ConsoleSide>) {
    let stdin = std::io::stdin();
    let mut input = stdin.lock();
    let mut line = String::new();
    let mut cycle = None;
    paint(&line);

    loop {
        let Some(key) = read_key(&mut input) else {
            break;
        };

        match key {
            Key::Enter => {
                println!();
                cycle = None;

                if !submit(&line, server.as_ref(), client.as_ref()) {
                    break;
                }

                line.clear();
                paint(&line);
            }
            Key::Backspace => {
                line.pop();
                cycle = None;
                paint(&line);
            }
            Key::Clear => {
                line.clear();
                cycle = None;
                paint(&line);
            }
            Key::Eof => {
                if line.is_empty() {
                    println!();

                    break;
                }
            }
            Key::Tab => {
                let matches = suggest_live(&line, server.as_ref(), client.as_ref());
                let applied = apply_completion(&line, &matches, &mut cycle);
                line = applied.line;

                if !applied.list.is_empty() {
                    println!();
                    print_list(&applied.list);
                }

                paint(&line);
            }
            Key::Text(text) => {
                line.push_str(&text);
                cycle = None;
                paint(&line);
            }
            Key::Ignore => {}
        }
    }
}

fn submit(line: &str, server: Option<&ConsoleSide>, client: Option<&ConsoleSide>) -> bool {
    let outcomes = dispatch(line, server, client);
    let mut idx = 0;

    while idx < outcomes.len() {
        let outcome = &outcomes[idx];
        println!("{}: {}", outcome.side, outcome.line);

        if let Some(detail) = &outcome.detail {
            println!("{detail}");
        }

        if let Some(err) = &outcome.error {
            eprintln!("{err}");
        }

        if outcome.quit {
            restore_term();
            std::process::exit(0);
        }

        idx += 1;
    }

    true
}

fn paint(line: &str) {
    print!("\r\x1b[2K] {line}");
    let _ = std::io::stdout().flush();
}

fn print_list(items: &[String]) {
    let mut idx = 0;

    while idx < items.len() {
        if idx > 0 {
            print!("  ");
        }

        print!("{}", items[idx]);
        idx += 1;
    }

    println!();
}

enum Key {
    Enter,
    Backspace,
    Clear,
    Eof,
    Tab,
    Text(String),
    Ignore,
}

fn read_key(input: &mut impl Read) -> Option<Key> {
    let first = read_byte(input)?;

    match first {
        b'\t' => Some(Key::Tab),
        b'\n' | b'\r' => Some(Key::Enter),
        0x7f | 0x08 => Some(Key::Backspace),
        0x04 => Some(Key::Eof),
        0x15 => Some(Key::Clear),
        0x1b => {
            discard_escape(input);

            Some(Key::Ignore)
        }
        byte if byte.is_ascii() && !byte.is_ascii_control() => {
            Some(Key::Text(char::from(byte).to_string()))
        }
        byte => {
            let width = utf8_width(byte);

            if width < 2 {
                return Some(Key::Ignore);
            }

            let mut buf = vec![byte];

            while buf.len() < width {
                let Some(next) = read_byte(input) else {
                    return Some(Key::Ignore);
                };

                buf.push(next);
            }

            match String::from_utf8(buf) {
                Ok(text) => Some(Key::Text(text)),
                Err(_) => Some(Key::Ignore),
            }
        }
    }
}

fn read_byte(input: &mut impl Read) -> Option<u8> {
    let mut buf = [0u8; 1];

    match input.read(&mut buf) {
        Ok(1) => Some(buf[0]),
        _ => None,
    }
}

fn utf8_width(byte: u8) -> usize {
    if byte & 0xE0 == 0xC0 {
        2
    } else if byte & 0xF0 == 0xE0 {
        3
    } else if byte & 0xF8 == 0xF0 {
        4
    } else {
        0
    }
}

fn discard_escape(input: &mut impl Read) {
    if !wait_byte(20) {
        return;
    }

    loop {
        let Some(byte) = read_byte(input) else {
            return;
        };

        if byte.is_ascii_alphabetic() || byte == b'~' {
            return;
        }

        if !wait_byte(0) {
            return;
        }
    }
}

struct Span {
    start: usize,
    prefix: String,
    command: bool,
    segment: String,
}

fn completion_span(line: &str) -> Span {
    let segment_at = last_segment_start(line);
    let segment = &line[segment_at..];
    let lead = segment.len() - segment.trim_start().len();
    let body = &segment[lead..];
    let segment_text = body.trim().to_string();

    if segment_text.is_empty() {
        return Span {
            start: line.len(),
            prefix: String::new(),
            command: true,
            segment: String::new(),
        };
    }

    if body.ends_with(char::is_whitespace) {
        return Span {
            start: line.len(),
            prefix: String::new(),
            command: false,
            segment: segment_text,
        };
    }

    let token_at = match body.rfind(char::is_whitespace) {
        Some(idx) => idx + body[idx..].chars().next().unwrap().len_utf8(),
        None => 0,
    };
    let prefix = body[token_at..].to_string();

    Span {
        start: segment_at + lead + token_at,
        prefix,
        command: tokenize(body.trim()).len() <= 1,
        segment: segment_text,
    }
}

fn last_segment_start(line: &str) -> usize {
    let bytes = line.as_bytes();
    let mut start = 0;
    let mut quoted = false;
    let mut idx = 0;

    while idx < bytes.len() {
        if bytes[idx] == b'"' {
            quoted = !quoted;
        } else if bytes[idx] == b';' && !quoted {
            start = idx + 1;
        }

        idx += 1;
    }

    start
}

struct Applied {
    line: String,
    list: Vec<String>,
}

fn suggest_live(
    line: &str,
    server: Option<&ConsoleSide>,
    client: Option<&ConsoleSide>,
) -> Vec<String> {
    suggest_with(line, server, client, None)
}

fn suggest_with(
    line: &str,
    server: Option<&ConsoleSide>,
    client: Option<&ConsoleSide>,
    local: Option<(&mlua::Lua, Realm)>,
) -> Vec<String> {
    let span = completion_span(line);
    let local_realm = local.map(|(_, realm)| realm);
    let server_rx = if server.is_some() && local_realm != Some(Realm::Server) {
        post_autocomplete(Realm::Server, &span.segment, &span.prefix)
    } else {
        None
    };
    let client_rx = if client.is_some() && local_realm != Some(Realm::Client) {
        post_autocomplete(Realm::Client, &span.segment, &span.prefix)
    } else {
        None
    };
    let mut server_extra = Vec::new();
    let mut client_extra = Vec::new();

    if local_realm == Some(Realm::Server) {
        if let Some((lua, _)) = local {
            server_extra = autocomplete_from_lua(lua, &span.segment, &span.prefix);
        }
    }

    if local_realm == Some(Realm::Client) {
        if let Some((lua, _)) = local {
            client_extra = autocomplete_from_lua(lua, &span.segment, &span.prefix);
        }
    }

    if let Some(rx) = server_rx {
        server_extra = rx
            .recv_timeout(Duration::from_millis(80))
            .unwrap_or_default();
    }

    if let Some(rx) = client_rx {
        client_extra = rx
            .recv_timeout(Duration::from_millis(80))
            .unwrap_or_default();
    }

    let mut extra = server_extra;
    extra.extend(client_extra);

    suggest(line, server, client, &extra)
}

fn suggest(
    line: &str,
    server: Option<&ConsoleSide>,
    client: Option<&ConsoleSide>,
    extra: &[String],
) -> Vec<String> {
    let span = completion_span(line);
    let mut found = Vec::new();

    if span.command {
        if let Some(server) = server {
            push_matching(&mut found, &builtin_names(server), &span.prefix);
        } else if let Some(client) = client {
            push_matching(&mut found, &builtin_names(client), &span.prefix);
        }

        if server.is_some() {
            if let Some(client) = client {
                push_matching(&mut found, &builtin_names(client), &span.prefix);
            }
        }
    } else if let Some((command, index)) = command_arg(line) {
        push_matching(
            &mut found,
            &builtin_args(&command, index, server, client),
            &span.prefix,
        );
    }

    push_matching(&mut found, extra, &span.prefix);
    found.sort();
    found.dedup();

    found
}

fn builtin_names(side: &ConsoleSide) -> Vec<String> {
    let mut names = Vec::new();
    let mut idx = 0;

    while idx < COMMANDS.len() {
        names.push(COMMANDS[idx].to_string());
        idx += 1;
    }

    for name in side.cvars.keys() {
        names.push(name.clone());
    }

    names
}

fn command_arg(line: &str) -> Option<(String, usize)> {
    let span = completion_span(line);

    if span.command {
        return None;
    }

    let tokens = tokenize(span.segment.trim());

    if tokens.is_empty() {
        return None;
    }

    let index = if span.prefix.is_empty() {
        tokens.len() - 1
    } else if tokens.len() >= 2 {
        tokens.len() - 2
    } else {
        return None;
    };

    Some((tokens[0].clone(), index))
}

fn builtin_args(
    command: &str,
    index: usize,
    server: Option<&ConsoleSide>,
    client: Option<&ConsoleSide>,
) -> Vec<String> {
    if index == 0 {
        if let Some(values) = cvar_values(command, server, client) {
            return values;
        }
    }

    match (command, index) {
        ("bind", 1) => actions(),
        ("demo_cam", 0) => vec![
            "first".to_string(),
            "chase".to_string(),
            "orbit".to_string(),
            "free".to_string(),
        ],
        ("demo_loop", 0) => vec!["0".to_string(), "1".to_string()],
        _ => Vec::new(),
    }
}

fn actions() -> Vec<String> {
    [
        "attack", "attack2", "use", "sprint", "walk", "duck", "jump", "reload", "forward", "back",
        "left", "right",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

fn cvar_values(
    name: &str,
    server: Option<&ConsoleSide>,
    client: Option<&ConsoleSide>,
) -> Option<Vec<String>> {
    let cvar = server
        .and_then(|side| side.cvars.get(name).cloned())
        .or_else(|| client.and_then(|side| side.cvars.get(name).cloned()))?;

    let value = cvar.value.lock().unwrap().clone();

    match value {
        ConVarValue::Bool(_) => Some(vec!["false".to_string(), "true".to_string()]),
        _ => None,
    }
}

fn push_matching(found: &mut Vec<String>, names: &[String], prefix: &str) {
    let mut idx = 0;

    while idx < names.len() {
        let name = &names[idx];
        idx += 1;

        if name.starts_with(prefix) && !found.iter().any(|have| have == name) {
            found.push(name.clone());
        }
    }
}

struct Cycle {
    before: String,
    options: Vec<String>,
    index: usize,
}

static WINDOW_CYCLE: Mutex<Option<Cycle>> = Mutex::new(None);

fn apply_completion(line: &str, matches: &[String], cycle: &mut Option<Cycle>) -> Applied {
    if let Some(state) = cycle.as_mut() {
        let current = format!("{}{}", state.before, state.options[state.index]);

        if line == current && state.options.len() > 1 {
            state.index = (state.index + 1) % state.options.len();
            let mut next = state.before.clone();
            next.push_str(&state.options[state.index]);

            return Applied {
                line: next,
                list: state.options.clone(),
            };
        }
    }

    *cycle = None;
    let span = completion_span(line);

    if matches.is_empty() {
        return Applied {
            line: line.to_string(),
            list: Vec::new(),
        };
    }

    if matches.len() == 1 {
        let mut next = String::new();
        next.push_str(&line[..span.start]);
        next.push_str(&matches[0]);
        next.push(' ');

        return Applied {
            line: next,
            list: Vec::new(),
        };
    }

    let before = line[..span.start].to_string();
    let mut next = before.clone();
    next.push_str(&matches[0]);
    *cycle = Some(Cycle {
        before,
        options: matches.to_vec(),
        index: 0,
    });

    Applied {
        line: next,
        list: matches.to_vec(),
    }
}

struct Pending {
    line: String,
    prefix: String,
    reply: Sender<Vec<String>>,
}

fn slot(realm: Realm) -> &'static Mutex<Option<Pending>> {
    static SERVER: Mutex<Option<Pending>> = Mutex::new(None);
    static CLIENT: Mutex<Option<Pending>> = Mutex::new(None);

    match realm {
        Realm::Client => &CLIENT,
        Realm::Server | Realm::Menu => &SERVER,
    }
}

fn post_autocomplete(
    realm: Realm,
    line: &str,
    prefix: &str,
) -> Option<mpsc::Receiver<Vec<String>>> {
    let (tx, rx) = mpsc::channel();
    let mut pending = slot(realm).lock().unwrap();
    *pending = Some(Pending {
        line: line.to_string(),
        prefix: prefix.to_string(),
        reply: tx,
    });

    Some(rx)
}

pub fn poll_autocomplete(realm: Realm, lua: &mlua::Lua) {
    let pending = {
        let mut slot = slot(realm).lock().unwrap();
        slot.take()
    };
    let Some(pending) = pending else {
        return;
    };
    let found = autocomplete_from_lua(lua, &pending.line, &pending.prefix);
    let _ = pending.reply.send(found);
}

fn autocomplete_from_lua(lua: &mlua::Lua, line: &str, prefix: &str) -> Vec<String> {
    let Ok(func) = lua.named_registry_value::<mlua::Function>(AUTOCOMPLETE_KEY) else {
        return Vec::new();
    };

    match func.call::<mlua::Value>((line.to_string(), prefix.to_string())) {
        Ok(mlua::Value::Table(table)) => {
            let mut found = Vec::new();

            for value in table.sequence_values::<String>() {
                let Ok(text) = value else {
                    continue;
                };

                if !text.is_empty() {
                    found.push(text);
                }
            }

            found
        }
        Ok(_) => Vec::new(),
        Err(err) => {
            log::error!("[console] autocomplete: {err}");

            Vec::new()
        }
    }
}

struct RawGuard;

impl Drop for RawGuard {
    fn drop(&mut self) {
        restore_term();
    }
}

static RAW: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
static mut ORIGINAL: Option<libc::termios> = None;

#[cfg(windows)]
static mut ORIGINAL: Option<u32> = None;

fn enable_raw() -> bool {
    #[cfg(unix)]
    {
        return enable_raw_unix();
    }

    #[cfg(windows)]
    {
        return enable_raw_windows();
    }

    #[cfg(not(any(unix, windows)))]
    {
        false
    }
}

fn restore_term() {
    if !RAW.swap(false, Ordering::SeqCst) {
        return;
    }

    #[cfg(unix)]
    unsafe {
        if let Some(prev) = ORIGINAL {
            libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &prev);
        }

        libc::signal(libc::SIGINT, libc::SIG_DFL);
    }

    #[cfg(windows)]
    unsafe {
        if let Some(mode) = ORIGINAL {
            SetConsoleMode(stdin_handle(), mode);
        }
    }
}

fn wait_byte(timeout_ms: i32) -> bool {
    #[cfg(unix)]
    {
        let mut fd = libc::pollfd {
            fd: libc::STDIN_FILENO,
            events: libc::POLLIN,
            revents: 0,
        };

        return unsafe { libc::poll(&mut fd, 1, timeout_ms) > 0 };
    }

    #[cfg(windows)]
    {
        let ms = timeout_ms.max(0) as u32;

        return unsafe { WaitForSingleObject(stdin_handle(), ms) == 0 };
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = timeout_ms;

        false
    }
}

#[cfg(unix)]
fn enable_raw_unix() -> bool {
    let fd = libc::STDIN_FILENO;

    if unsafe { libc::isatty(fd) } == 0 {
        return false;
    }

    let mut prev: libc::termios = unsafe { std::mem::zeroed() };

    if unsafe { libc::tcgetattr(fd, &mut prev) } != 0 {
        return false;
    }

    let mut raw = prev;
    raw.c_lflag &= !(libc::ECHO | libc::ICANON);
    raw.c_cc[libc::VMIN] = 1;
    raw.c_cc[libc::VTIME] = 0;

    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &raw) } != 0 {
        return false;
    }

    unsafe {
        ORIGINAL = Some(prev);
        libc::signal(libc::SIGINT, on_int as *const () as libc::sighandler_t);
    }
    RAW.store(true, Ordering::SeqCst);

    true
}

#[cfg(unix)]
unsafe extern "C" fn on_int(_sig: libc::c_int) {
    restore_term();
    libc::signal(libc::SIGINT, libc::SIG_DFL);
    libc::raise(libc::SIGINT);
}

#[cfg(windows)]
fn enable_raw_windows() -> bool {
    let handle = stdin_handle();
    let mut mode = 0u32;

    if unsafe { GetConsoleMode(handle, &mut mode) } == 0 {
        return false;
    }

    let next = mode & !2 & !4;

    if unsafe { SetConsoleMode(handle, next) } == 0 {
        return false;
    }

    unsafe {
        ORIGINAL = Some(mode);
    }
    RAW.store(true, Ordering::SeqCst);

    true
}

#[cfg(windows)]
fn stdin_handle() -> *mut std::ffi::c_void {
    unsafe { GetStdHandle(0xFFFFFFF6) }
}

#[cfg(windows)]
extern "system" {
    fn GetStdHandle(nstd: u32) -> *mut std::ffi::c_void;
    fn GetConsoleMode(handle: *mut std::ffi::c_void, mode: *mut u32) -> i32;
    fn SetConsoleMode(handle: *mut std::ffi::c_void, mode: u32) -> i32;
    fn WaitForSingleObject(handle: *mut std::ffi::c_void, millis: u32) -> u32;
}

fn dispatch(
    line: &str,
    server: Option<&ConsoleSide>,
    client: Option<&ConsoleSide>,
) -> Vec<Outcome> {
    let commented = strip_comment(line);
    let parts = split_commands(commented);
    let mut outcomes = Vec::new();
    let mut idx = 0;

    while idx < parts.len() {
        let part = parts[idx].trim();
        idx += 1;

        if part.is_empty() {
            continue;
        }

        let outcome = run_part(part, server, client);
        let quit = outcome.quit;
        outcomes.push(outcome);

        if quit {
            break;
        }
    }

    outcomes
}

fn run_part(part: &str, server: Option<&ConsoleSide>, client: Option<&ConsoleSide>) -> Outcome {
    let previous = demo::realm();
    let outcome = run_part_realm(part, server, client);
    demo::set_realm(previous);

    outcome
}

fn run_part_realm(
    part: &str,
    server: Option<&ConsoleSide>,
    client: Option<&ConsoleSide>,
) -> Outcome {
    if let Some(server) = server {
        demo::bind_realm(Realm::Server);

        match try_side(server, part) {
            Hit::Done(Err(err)) if client.is_some() && err == "demo playback is client only" => {}
            Hit::Local if holds_cvar(client, part) => {}
            Hit::Local => return done("server", part, assign_line(server, part)),
            Hit::Done(result) => return done("server", part, result),
            Hit::Quit => return quit_outcome("server", part),
            Hit::Miss => {}
        }
    }

    if let Some(client) = client {
        demo::bind_realm(Realm::Client);

        match try_side(client, part) {
            Hit::Done(result) => return done("client", part, result),
            Hit::Local => return done("client", part, assign_line(client, part)),
            Hit::Quit => return quit_outcome("client", part),
            Hit::Miss => return unknown("client", part),
        }
    }

    unknown("server", part)
}

fn try_side(side: &ConsoleSide, line: &str) -> Hit {
    let tokens = tokenize(strip_comment(line).trim());

    if tokens.is_empty() {
        return Hit::Done(Ok(None));
    }

    if tokens[0] == "quit" || tokens[0] == "exit" {
        return Hit::Quit;
    }

    let exec_result = {
        let mut binds = side.binds.lock().unwrap();
        exec_line(line, &mut binds)
    };

    match exec_result {
        Ok(()) => Hit::Done(Ok(None)),
        Err(err) if err == format!("unknown command '{}'", tokens[0]) => {
            let Some(cvar) = side.cvars.get(&tokens[0]) else {
                return Hit::Miss;
            };

            if !cvar.is_replicated_to_clients {
                return Hit::Local;
            }

            Hit::Done(assign_cvar(cvar, &tokens[1..]))
        }
        Err(err) => Hit::Done(Err(err)),
    }
}

fn holds_cvar(side: Option<&ConsoleSide>, line: &str) -> bool {
    let Some(side) = side else {
        return false;
    };

    let tokens = tokenize(strip_comment(line).trim());
    let Some(name) = tokens.first() else {
        return false;
    };

    side.cvars.contains_key(name)
}

fn assign_line(side: &ConsoleSide, line: &str) -> Result<Option<String>, String> {
    let tokens = tokenize(strip_comment(line).trim());
    let Some(name) = tokens.first() else {
        return Ok(None);
    };
    let Some(cvar) = side.cvars.get(name) else {
        return Err(format!("unknown command '{name}'"));
    };

    assign_cvar(cvar, &tokens[1..])
}

fn assign_cvar(cvar: &ConVar, args: &[String]) -> Result<Option<String>, String> {
    if args.is_empty() {
        let value = cvar.value.lock().unwrap();

        return Ok(Some(format_value(&value)));
    }

    let current = cvar.value.lock().unwrap().clone();
    let next = match current {
        ConVarValue::Integer(_) => {
            let text = one_arg(&cvar.name, args)?;
            let parsed = text
                .parse::<i64>()
                .map_err(|_| format!("'{text}' is not an integer"))?;

            ConVarValue::Integer(parsed)
        }
        ConVarValue::Float(_) => {
            let text = one_arg(&cvar.name, args)?;
            let parsed = text
                .parse::<f64>()
                .map_err(|_| format!("'{text}' is not a number"))?;

            if !parsed.is_finite() {
                return Err(format!("'{text}' is not a number"));
            }

            ConVarValue::Float(parsed)
        }
        ConVarValue::Bool(_) => {
            let text = one_arg(&cvar.name, args)?;

            ConVarValue::Bool(parse_bool(text)?)
        }
        ConVarValue::String(_) => ConVarValue::String(args.join(" ")),
    };
    cvar.set_value(next);

    Ok(None)
}

fn one_arg<'a>(name: &str, args: &'a [String]) -> Result<&'a str, String> {
    if args.len() != 1 {
        return Err(format!("usage: {name} <value>"));
    }

    Ok(args[0].as_str())
}

fn parse_bool(text: &str) -> Result<bool, String> {
    match text.to_ascii_lowercase().as_str() {
        "true" | "1" => Ok(true),
        "false" | "0" => Ok(false),
        _ => Err(format!("'{text}' is not a bool")),
    }
}

fn format_value(value: &ConVarValue) -> String {
    match value {
        ConVarValue::Integer(value) => value.to_string(),
        ConVarValue::Float(value) => value.to_string(),
        ConVarValue::Bool(value) => value.to_string(),
        ConVarValue::String(value) => value.clone(),
    }
}

fn split_commands(line: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut chars = line.chars();
    let mut quoted = false;

    while let Some(ch) = chars.next() {
        if ch == '"' {
            quoted = !quoted;
            current.push(ch);

            continue;
        }

        if ch == ';' && !quoted {
            parts.push(std::mem::take(&mut current));

            continue;
        }

        current.push(ch);
    }

    parts.push(current);

    parts
}

fn done(side: &'static str, line: &str, result: Result<Option<String>, String>) -> Outcome {
    match result {
        Ok(detail) => Outcome {
            side,
            line: line.to_string(),
            detail,
            error: None,
            quit: false,
        },
        Err(err) => Outcome {
            side,
            line: line.to_string(),
            detail: None,
            error: Some(err),
            quit: false,
        },
    }
}

fn quit_outcome(side: &'static str, line: &str) -> Outcome {
    Outcome {
        side,
        line: line.to_string(),
        detail: None,
        error: None,
        quit: true,
    }
}

fn unknown(side: &'static str, line: &str) -> Outcome {
    let tokens = tokenize(line);
    let name = tokens.first().map(String::as_str).unwrap_or(line);

    Outcome {
        side,
        line: line.to_string(),
        detail: None,
        error: Some(format!("unknown command '{name}'")),
        quit: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::{Action, Binding};
    use crate::platform::KeyCode;

    fn cvar(name: &str, value: ConVarValue, replicated: bool) -> Arc<ConVar> {
        Arc::new(ConVar::new(name, value, "", Some(false), Some(replicated)))
    }

    fn side(entries: Vec<(&str, ConVarValue)>) -> ConsoleSide {
        filled(entries, false)
    }

    fn replicated_side(entries: Vec<(&str, ConVarValue)>) -> ConsoleSide {
        filled(entries, true)
    }

    fn filled(entries: Vec<(&str, ConVarValue)>, replicated: bool) -> ConsoleSide {
        let mut cvars = HashMap::new();

        for (name, value) in entries {
            cvars.insert(name.to_string(), cvar(name, value, replicated));
        }

        ConsoleSide {
            cvars: Arc::new(cvars),
            binds: Arc::new(Mutex::new(Binds::new())),
        }
    }

    fn float_of(side: &ConsoleSide, name: &str) -> f64 {
        let var = side.cvars.get(name).unwrap();
        match &*var.value.lock().unwrap() {
            ConVarValue::Float(value) => *value,
            _ => panic!("{name} is not a float"),
        }
    }

    #[test]
    fn server_command_stays_on_the_server() {
        let server = side(Vec::new());
        let client = side(Vec::new());
        let outcomes = dispatch("bind e use", Some(&server), Some(&client));

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].side, "server");
        assert!(outcomes[0].error.is_none());
        assert_eq!(
            server
                .binds
                .lock()
                .unwrap()
                .get(Binding::Key(KeyCode::KeyE)),
            Some(Action::Use)
        );
        assert_eq!(
            client
                .binds
                .lock()
                .unwrap()
                .get(Binding::Key(KeyCode::KeyE)),
            None
        );
    }

    #[test]
    fn server_error_does_not_fall_through() {
        let server = side(Vec::new());
        let client = side(Vec::new());
        let outcomes = dispatch("bind e", Some(&server), Some(&client));

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].side, "server");
        assert!(outcomes[0].error.is_some());
        assert_eq!(
            client
                .binds
                .lock()
                .unwrap()
                .get(Binding::Key(KeyCode::KeyE)),
            None
        );
    }

    #[test]
    fn client_only_demo_commands_run_on_the_client() {
        let server = side(Vec::new());
        let client = side(Vec::new());
        let outcomes = dispatch("playdemo demo", Some(&server), Some(&client));

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].side, "client");
        assert!(outcomes[0].error.is_none());

        let queued = demo::drain(Realm::Client);
        assert!(matches!(
            queued.first(),
            Some(demo::DemoCommand::Play { name }) if name == "demo"
        ));
    }

    #[test]
    fn client_only_error_stays_without_a_client() {
        let server = side(Vec::new());
        let outcomes = dispatch("playdemo demo", Some(&server), None);

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].side, "server");
        assert_eq!(
            outcomes[0].error.as_deref(),
            Some("demo playback is client only")
        );
    }

    #[test]
    fn server_cvar_is_set_on_the_server() {
        let server = replicated_side(vec![("sv_gravity", ConVarValue::Float(24.0))]);
        let client = replicated_side(vec![("sv_gravity", ConVarValue::Float(24.0))]);
        let outcomes = dispatch("sv_gravity 20", Some(&server), Some(&client));

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].side, "server");
        assert!(outcomes[0].error.is_none());
        assert_eq!(float_of(&server, "sv_gravity"), 20.0);
        assert_eq!(float_of(&client, "sv_gravity"), 24.0);
    }

    #[test]
    fn client_local_cvar_is_set_on_the_client() {
        let server = side(vec![("snd_volume", ConVarValue::Float(1.0))]);
        let client = side(vec![("snd_volume", ConVarValue::Float(1.0))]);
        let outcomes = dispatch("snd_volume 0.25", Some(&server), Some(&client));

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].side, "client");
        assert!(outcomes[0].error.is_none());
        assert_eq!(float_of(&client, "snd_volume"), 0.25);
        assert_eq!(float_of(&server, "snd_volume"), 1.0);
    }

    #[test]
    fn local_cvar_without_a_client_copy_stays_on_the_server() {
        let server = side(vec![("snd_volume", ConVarValue::Float(1.0))]);
        let client = side(Vec::new());
        let outcomes = dispatch("snd_volume 0.25", Some(&server), Some(&client));

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].side, "server");
        assert!(outcomes[0].error.is_none());
        assert_eq!(float_of(&server, "snd_volume"), 0.25);
    }

    #[test]
    fn missing_server_cvar_is_set_on_the_client() {
        let server = side(Vec::new());
        let client = side(vec![("cl_only", ConVarValue::Float(1.0))]);
        let outcomes = dispatch("cl_only 3", Some(&server), Some(&client));

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].side, "client");
        assert!(outcomes[0].error.is_none());
        assert_eq!(float_of(&client, "cl_only"), 3.0);
        assert!(!server.cvars.contains_key("cl_only"));
    }

    #[test]
    fn missing_name_is_not_created() {
        let server = side(Vec::new());
        let client = side(Vec::new());
        let outcomes = dispatch("newcvar 1", Some(&server), Some(&client));

        assert_eq!(outcomes.len(), 1);
        assert_eq!(outcomes[0].side, "client");
        assert_eq!(
            outcomes[0].error.as_deref(),
            Some("unknown command 'newcvar'")
        );
        assert!(server.cvars.is_empty());
        assert!(client.cvars.is_empty());
    }

    #[test]
    fn semicolon_runs_each_command_on_the_server() {
        let server = side(Vec::new());
        let client = side(Vec::new());
        let outcomes = dispatch("bind e use; bind r reload", Some(&server), Some(&client));

        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].side, "server");
        assert_eq!(outcomes[1].side, "server");
        assert_eq!(
            server
                .binds
                .lock()
                .unwrap()
                .get(Binding::Key(KeyCode::KeyR)),
            Some(Action::Reload)
        );
    }

    #[test]
    fn quoted_semicolon_stays_one_command() {
        let server = side(Vec::new());
        let client = side(Vec::new());
        let outcomes = dispatch("bind \"e;f\" use", Some(&server), Some(&client));

        assert_eq!(outcomes.len(), 1);
    }

    #[test]
    fn quit_stops_the_rest_of_the_line() {
        let server = side(Vec::new());
        let client = side(Vec::new());
        let outcomes = dispatch(
            "bind e use; quit; bind r reload",
            Some(&server),
            Some(&client),
        );

        assert_eq!(outcomes.len(), 2);
        assert!(outcomes[1].quit);
        assert_eq!(outcomes[1].side, "server");
        assert_eq!(
            server
                .binds
                .lock()
                .unwrap()
                .get(Binding::Key(KeyCode::KeyR)),
            None
        );
    }

    #[test]
    fn tab_completes_one_command() {
        let server = side(Vec::new());
        let client = side(Vec::new());
        let matches = suggest("rec", Some(&server), Some(&client), &[]);
        let applied = apply_completion("rec", &matches, &mut None);

        assert_eq!(applied.line, "record ");
        assert!(applied.list.is_empty());
    }

    #[test]
    fn tab_lists_commands_that_share_a_prefix() {
        let server = side(Vec::new());
        let client = side(Vec::new());
        let mut cycle = None;
        let first = suggest("d", Some(&server), Some(&client), &[]);
        let grown = apply_completion("d", &first, &mut cycle);

        assert_eq!(grown.line, "demo_cam");
        assert!(grown.list.iter().any(|name| name == "demo_pause"));

        let listed = apply_completion(&grown.line, &[], &mut cycle);

        assert_eq!(listed.line, "demo_loop");
        assert!(listed.list.iter().any(|name| name == "demo_pause"));
    }

    #[test]
    fn tab_completes_a_server_cvar() {
        let server = side(vec![("snd_volume", ConVarValue::Float(1.0))]);
        let client = side(Vec::new());
        let matches = suggest("snd", Some(&server), Some(&client), &[]);
        let applied = apply_completion("snd", &matches, &mut None);

        assert_eq!(applied.line, "snd_volume ");
    }

    #[test]
    fn tab_completes_a_client_only_cvar() {
        let server = side(Vec::new());
        let client = side(vec![("cl_only", ConVarValue::Float(1.0))]);
        let matches = suggest("cl", Some(&server), Some(&client), &[]);
        let applied = apply_completion("cl", &matches, &mut None);

        assert_eq!(applied.line, "cl_only ");
    }

    #[test]
    fn tab_completes_the_command_after_a_semicolon() {
        let server = side(Vec::new());
        let client = side(Vec::new());
        let line = "bind e use; rec";
        let matches = suggest(line, Some(&server), Some(&client), &[]);
        let applied = apply_completion(line, &matches, &mut None);

        assert_eq!(applied.line, "bind e use; record ");
    }

    #[test]
    fn lua_names_complete_arguments() {
        let server = side(Vec::new());
        let client = side(Vec::new());
        let extra = vec!["hall".to_string(), "hallway".to_string()];
        let mut cycle = None;
        let matches = suggest("playdemo h", Some(&server), Some(&client), &extra);
        let applied = apply_completion("playdemo h", &matches, &mut cycle);

        assert_eq!(applied.line, "playdemo hall");
        assert_eq!(
            applied.list,
            vec!["hall".to_string(), "hallway".to_string()]
        );

        let listed = apply_completion(&applied.line, &[], &mut cycle);

        assert_eq!(listed.line, "playdemo hallway");
    }

    #[test]
    fn tab_completes_builtin_arguments() {
        let server = side(Vec::new());
        let client = side(Vec::new());
        let matches = suggest("demo_cam ", Some(&server), Some(&client), &[]);
        let applied = apply_completion("demo_cam ", &matches, &mut None);

        assert_eq!(applied.line, "demo_cam chase");
        assert!(applied.list.iter().any(|name| name == "free"));

        let actions = suggest("bind e ", Some(&server), Some(&client), &[]);
        let filled = apply_completion("bind e ", &actions, &mut None);

        assert_eq!(filled.line, "bind e attack");
        assert!(filled.list.iter().any(|name| name == "use"));
    }

    #[test]
    fn lua_callback_is_optional() {
        let lua = mlua::Lua::new();
        let missing = post_autocomplete(Realm::Server, "rec", "rec").unwrap();
        poll_autocomplete(Realm::Server, &lua);
        assert!(missing.recv().unwrap().is_empty());

        let binds = Arc::new(Mutex::new(Binds::new()));
        crate::script::libs::console::register_console_lib(&lua, binds, Realm::Client);
        lua.load(
            r#"console.autocomplete(function(line, prefix) return {"hall", "hallway", "other"} end)"#,
        )
        .exec()
        .unwrap();
        let rx = post_autocomplete(Realm::Client, "playdemo h", "h").unwrap();
        poll_autocomplete(Realm::Client, &lua);
        let extra = rx.recv().unwrap();
        let server = side(Vec::new());
        let matches = suggest("playdemo h", Some(&server), None, &extra);

        assert_eq!(matches, vec!["hall".to_string(), "hallway".to_string()]);
    }

    #[test]
    fn dispatch_restores_the_demo_realm() {
        demo::bind_realm(Realm::Client);
        let server = side(Vec::new());
        let client = side(Vec::new());
        let _ = dispatch("bind e use", Some(&server), Some(&client));

        assert!(matches!(demo::realm(), Some(Realm::Client)));
    }
}
