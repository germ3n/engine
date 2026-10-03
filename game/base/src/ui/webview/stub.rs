use super::{Mods, RawFrame, WebHost};

struct StubView {
    width: u32,
    height: u32,
    dirty: bool,
}

pub struct StubHost {
    next: u64,
    views: Vec<(u64, StubView)>,
    messages: Vec<(u64, String)>,
}

impl StubHost {
    pub fn new() -> Self {
        Self {
            next: 0,
            views: Vec::new(),
            messages: Vec::new(),
        }
    }

    fn find(&mut self, id: u64) -> Option<&mut StubView> {
        let mut idx = 0;

        while idx < self.views.len() {
            if self.views[idx].0 == id {
                return Some(&mut self.views[idx].1);
            }

            idx += 1;
        }

        None
    }
}

impl WebHost for StubHost {
    fn create(&mut self, width: u32, height: u32) -> u64 {
        self.next += 1;
        self.views.push((
            self.next,
            StubView {
                width,
                height,
                dirty: false,
            },
        ));

        self.next
    }

    fn remove(&mut self, id: u64) {
        let mut idx = 0;

        while idx < self.views.len() {
            if self.views[idx].0 == id {
                self.views.remove(idx);

                return;
            }

            idx += 1;
        }
    }

    fn load_html(&mut self, id: u64, _html: &str) {
        if let Some(view) = self.find(id) {
            view.dirty = true;
        }
    }

    fn load_url(&mut self, id: u64, _url: &str) {
        if let Some(view) = self.find(id) {
            view.dirty = true;
        }
    }

    fn resize(&mut self, id: u64, width: u32, height: u32) {
        if let Some(view) = self.find(id) {
            view.width = width.max(1);
            view.height = height.max(1);
            view.dirty = true;
        }
    }

    fn run_js(&mut self, id: u64, _code: &str) {
        if let Some(view) = self.find(id) {
            view.dirty = true;
        }
    }

    fn mouse_move(&mut self, _id: u64, _x: f32, _y: f32) {}

    fn mouse_button(&mut self, _id: u64, _button: i32, _down: bool) {}

    fn mouse_wheel(&mut self, _id: u64, _dx: f32, _dy: f32) {}

    fn key(&mut self, _id: u64, _name: &str, _down: bool, _repeat: bool, _mods: Mods) {}

    fn text(&mut self, _id: u64, _text: &str) {}

    fn focus(&mut self, _id: u64, _on: bool) {}

    fn pump(&mut self) {}

    fn poll_frames(&mut self) -> Vec<RawFrame> {
        let mut out = Vec::new();
        let mut idx = 0;

        while idx < self.views.len() {
            let (id, view) = &mut self.views[idx];
            idx += 1;

            if !view.dirty {
                continue;
            }

            view.dirty = false;
            let mut bytes = vec![0u8; (view.width as usize) * (view.height as usize) * 4];

            if bytes.len() >= 4 {
                bytes[0] = 255;
                bytes[3] = 255;
            }

            out.push(RawFrame {
                id: *id,
                width: view.width,
                height: view.height,
                bytes,
            });
        }

        out
    }

    fn poll_messages(&mut self) -> Vec<(u64, String)> {
        std::mem::take(&mut self.messages)
    }

    fn debug_message(&mut self, id: u64, text: &str) {
        self.messages.push((id, text.to_string()));
    }
}
