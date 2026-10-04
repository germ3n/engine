#[cfg(target_os = "macos")]
mod macos;
mod stub;

use crate::script::engine::{DrawCommand, RenderQueue};
use crate::ui::gfx;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};

static USE_PLATFORM: AtomicBool = AtomicBool::new(false);

pub fn prefer_platform() {
    USE_PLATFORM.store(true, Ordering::Relaxed);
}

#[derive(Clone, Copy, Default)]
pub struct Mods {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub command: bool,
}

pub struct RawFrame {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

pub struct Upload {
    pub texture: u32,
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

trait WebHost: Send {
    fn create(&mut self, width: u32, height: u32) -> u64;
    fn remove(&mut self, id: u64);
    fn load_html(&mut self, id: u64, html: &str);
    fn load_url(&mut self, id: u64, url: &str);
    fn resize(&mut self, id: u64, width: u32, height: u32);
    fn run_js(&mut self, id: u64, code: &str);
    fn mouse_move(&mut self, id: u64, x: f32, y: f32);
    fn mouse_button(&mut self, id: u64, button: i32, down: bool);
    fn mouse_wheel(&mut self, id: u64, dx: f32, dy: f32);
    fn key(&mut self, id: u64, name: &str, down: bool, repeat: bool, mods: Mods);
    fn text(&mut self, id: u64, text: &str);
    fn focus(&mut self, id: u64, on: bool);
    fn pump(&mut self);
    fn poll_frames(&mut self) -> Vec<RawFrame>;
    fn poll_messages(&mut self) -> Vec<(u64, String)>;
    fn debug_message(&mut self, id: u64, text: &str);
}

struct Slot {
    texture: u32,
    last: Vec<u8>,
}

pub struct Bank {
    host: Box<dyn WebHost>,
    views: HashMap<u64, Slot>,
}

impl Bank {
    pub fn open() -> Self {
        Self {
            host: open_host(),
            views: HashMap::new(),
        }
    }

    pub fn spawn(&mut self, queue: &RenderQueue, width: u32, height: u32) -> Option<(u64, u32)> {
        let (width, height) = limit(width, height);
        let bytes = vec![0u8; (width as usize) * (height as usize) * 4];
        let id = self.host.create(width, height);

        if id == 0 {
            return None;
        }

        let mut state = queue.lock().expect("Couldn't lock render queue");
        let texture = state.book.alloc(gfx::KIND_TEXTURE);

        if texture == 0 {
            drop(state);
            self.host.remove(id);

            return None;
        }

        state.commands.push(DrawCommand::CreateImage {
            id: texture,
            width,
            height,
            bytes: bytes.clone(),
        });
        drop(state);
        self.views.insert(
            id,
            Slot {
                texture,
                last: bytes,
            },
        );

        Some((id, texture))
    }

    pub fn remove(&mut self, id: u64, queue: &RenderQueue) {
        self.host.remove(id);
        let Some(slot) = self.views.remove(&id) else {
            return;
        };
        let mut state = queue.lock().expect("Couldn't lock render queue");

        if state.book.doom(slot.texture) {
            state.commands.push(DrawCommand::Free { id: slot.texture });
        }
    }

    pub fn load_html(&mut self, id: u64, html: &str) {
        if self.views.contains_key(&id) {
            self.host.load_html(id, html);
        }
    }

    pub fn load_url(&mut self, id: u64, url: &str) {
        if self.views.contains_key(&id) {
            self.host.load_url(id, url);
        }
    }

    pub fn resize(&mut self, id: u64, width: u32, height: u32) {
        if !self.views.contains_key(&id) {
            return;
        }

        let (width, height) = limit(width, height);
        self.host.resize(id, width, height);
    }

    pub fn run_js(&mut self, id: u64, code: &str) {
        if self.views.contains_key(&id) {
            self.host.run_js(id, code);
        }
    }

    pub fn mouse_move(&mut self, id: u64, x: f32, y: f32) {
        if self.views.contains_key(&id) {
            self.host.mouse_move(id, x, y);
        }
    }

    pub fn mouse_button(&mut self, id: u64, button: i32, down: bool) {
        if self.views.contains_key(&id) {
            self.host.mouse_button(id, button, down);
        }
    }

    pub fn mouse_wheel(&mut self, id: u64, dx: f32, dy: f32) {
        if self.views.contains_key(&id) {
            self.host.mouse_wheel(id, dx, dy);
        }
    }

    pub fn key(&mut self, id: u64, name: &str, down: bool, repeat: bool, mods: Mods) {
        if self.views.contains_key(&id) {
            self.host.key(id, name, down, repeat, mods);
        }
    }

    pub fn text(&mut self, id: u64, text: &str) {
        if self.views.contains_key(&id) {
            self.host.text(id, text);
        }
    }

    pub fn focus(&mut self, id: u64, on: bool) {
        if self.views.contains_key(&id) {
            self.host.focus(id, on);
        }
    }

    #[allow(dead_code)]
    pub fn debug_message(&mut self, id: u64, text: &str) {
        if self.views.contains_key(&id) {
            self.host.debug_message(id, text);
        }
    }

    pub fn drain(&mut self) -> (Vec<Upload>, Vec<(u64, String)>) {
        self.host.pump();
        let polled = self.host.poll_frames();
        let mut uploads = Vec::new();

        for frame in polled {
            let Some(slot) = self.views.get_mut(&frame.id) else {
                continue;
            };

            if slot.last == frame.bytes {
                continue;
            }

            slot.last.clone_from(&frame.bytes);
            uploads.push(Upload {
                texture: slot.texture,
                width: frame.width,
                height: frame.height,
                bytes: frame.bytes,
            });
        }

        let incoming = self.host.poll_messages();
        let mut messages = Vec::new();

        for (id, text) in incoming {
            if self.views.contains_key(&id) {
                messages.push((id, text));
            }
        }

        (uploads, messages)
    }
}

fn open_host() -> Box<dyn WebHost> {
    if USE_PLATFORM.load(Ordering::Relaxed) {
        #[cfg(target_os = "macos")]
        {
            return Box::new(macos::MacHost::new());
        }

        #[cfg(not(target_os = "macos"))]
        {
            log::info!("[webview] no system webview on this platform");
        }
    }

    Box::new(stub::StubHost::new())
}

fn limit(width: u32, height: u32) -> (u32, u32) {
    (width.clamp(1, 4096), height.clamp(1, 4096))
}
