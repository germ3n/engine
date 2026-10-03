use crate::script::engine::RenderQueue;
use crate::script::libs::input::Pointer;
use crate::ui::webview::{Bank, Mods};
use mlua::prelude::LuaUserDataMethods;
use mlua::{Lua, UserData};
use r#macro::document;
use std::sync::{Arc, Mutex};

struct WebApp {
    bank: Arc<Mutex<Bank>>,
    queue: RenderQueue,
    pointer: Arc<Mutex<Pointer>>,
}

pub struct LuaView {
    id: u64,
    texture: u32,
    bank: Arc<Mutex<Bank>>,
    queue: RenderQueue,
    alive: bool,
}

impl LuaView {
    fn close(&mut self) {
        if !self.alive {
            return;
        }

        self.alive = false;
        self.bank
            .lock()
            .expect("Couldn't lock webview")
            .remove(self.id, &self.queue);
    }
}

impl Drop for LuaView {
    fn drop(&mut self) {
        self.close();
    }
}

impl UserData for LuaView {
    fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
        methods.add_method_mut("load_html", |_, this, html: String| {
            if this.alive {
                this.bank.lock().expect("Couldn't lock webview").load_html(this.id, &html);
            }

            Ok(())
        });
        methods.add_method_mut("load_url", |_, this, url: String| {
            if this.alive {
                this.bank.lock().expect("Couldn't lock webview").load_url(this.id, &url);
            }

            Ok(())
        });
        methods.add_method_mut("resize", |_, this, (width, height): (u32, u32)| {
            if this.alive {
                this.bank
                    .lock()
                    .expect("Couldn't lock webview")
                    .resize(this.id, width, height);
            }

            Ok(())
        });
        methods.add_method_mut("run_js", |_, this, code: String| {
            if this.alive {
                this.bank.lock().expect("Couldn't lock webview").run_js(this.id, &code);
            }

            Ok(())
        });
        methods.add_method("texture", |_, this, ()| {
            if !this.alive {
                return Ok(0u32);
            }

            Ok(this.texture)
        });
        methods.add_method("id", |_, this, ()| Ok(this.id as i64));
        methods.add_method_mut("remove", |lua, this, ()| {
            clear_listener(lua, this.id);
            this.close();

            Ok(())
        });
        methods.add_method("on_message", |lua, this, func: mlua::Function| {
            if !this.alive {
                return Ok(());
            }

            let table: mlua::Table = lua.globals().get("webview")?;
            let listeners: mlua::Table = table.get("_listeners")?;
            listeners.set(this.id as i64, func)?;

            Ok(())
        });
        methods.add_method_mut("mouse_move", |_, this, (x, y): (f64, f64)| {
            if this.alive {
                this.bank
                    .lock()
                    .expect("Couldn't lock webview")
                    .mouse_move(this.id, x as f32, y as f32);
            }

            Ok(())
        });
        methods.add_method_mut("mouse_button", |_, this, (button, down): (i32, bool)| {
            if this.alive {
                this.bank
                    .lock()
                    .expect("Couldn't lock webview")
                    .mouse_button(this.id, button, down);
            }

            Ok(())
        });
        methods.add_method_mut("mouse_wheel", |_, this, (x, y): (f64, f64)| {
            if this.alive {
                this.bank
                    .lock()
                    .expect("Couldn't lock webview")
                    .mouse_wheel(this.id, x as f32, y as f32);
            }

            Ok(())
        });
        methods.add_method_mut("key", |lua, this, (name, down, repeat): (String, bool, Option<bool>)| {
            if this.alive {
                let mods = current_mods(lua);
                this.bank.lock().expect("Couldn't lock webview").key(
                    this.id,
                    &name,
                    down,
                    repeat.unwrap_or(false),
                    mods,
                );
            }

            Ok(())
        });
        methods.add_method_mut("text", |_, this, text: String| {
            if this.alive {
                this.bank.lock().expect("Couldn't lock webview").text(this.id, &text);
            }

            Ok(())
        });
        methods.add_method_mut("focus", |_, this, on: bool| {
            if this.alive {
                this.bank.lock().expect("Couldn't lock webview").focus(this.id, on);
            }

            Ok(())
        });
    }
}

fn current_mods(lua: &Lua) -> Mods {
    let Some(app) = lua.app_data_ref::<WebApp>() else {
        return Mods::default();
    };
    let pointer = app.pointer.lock().expect("Couldn't lock pointer");

    Mods {
        shift: pointer.shift,
        control: pointer.control,
        alt: pointer.alt,
        command: pointer.super_key,
    }
}

fn clear_listener(lua: &Lua, id: u64) {
    let Ok(table) = lua.globals().get::<mlua::Table>("webview") else {
        return;
    };
    let Ok(listeners) = table.get::<mlua::Table>("_listeners") else {
        return;
    };
    let _ = listeners.set(id as i64, mlua::Value::Nil);
}

#[document(
    kind = "library",
    name = "webview",
    realm = "client",
    summary = "System webview painted into a surface texture. Pages can post strings to Lua with window.engine.post."
)]
fn webview_lib() {}

#[document(
    parent = "webview",
    name = "create",
    kind = "function",
    realm = "client",
    summary = "Opens a webview and a texture of the given size. The texture updates when the page paints.",
    params = {
        width = { ty = "number", desc = "Width in pixels, clamped to 1..4096." },
        height = { ty = "number", desc = "Height in pixels, clamped to 1..4096." },
    },
    returns = { ty = "WebView", desc = "The view, or nil if it could not be created." },
    example = "local page = gui.create(\"Html\")\npage:set_pos(80, 80)\npage:set_size(640, 360)\npage:load_html(\"<html><body style='background:#222;color:#fff'>Hello</body></html>\")",
)]
fn webview_create() {}

#[document(
    kind = "class",
    name = "WebView",
    realm = "client",
    summary = "One HTML page. texture is a surface texture id. JS window.engine.post(text) calls the on_message callback."
)]
fn webview_class() {}

#[document(
    parent = "WebView",
    name = "load_html",
    kind = "method",
    realm = "client",
    summary = "Loads an HTML document.",
    params = {
        html = { ty = "string", desc = "Document source." },
    },
)]
fn webview_load_html() {}

#[document(
    parent = "WebView",
    name = "load_url",
    kind = "method",
    realm = "client",
    summary = "Loads a URL.",
    params = {
        url = { ty = "string", desc = "http or https URL." },
    },
)]
fn webview_load_url() {}

#[document(
    parent = "WebView",
    name = "resize",
    kind = "method",
    realm = "client",
    summary = "Changes the page size. The next paint uploads a texture of the new size.",
    params = {
        width = { ty = "number", desc = "Width in pixels." },
        height = { ty = "number", desc = "Height in pixels." },
    },
)]
fn webview_resize() {}

#[document(
    parent = "WebView",
    name = "run_js",
    kind = "method",
    realm = "client",
    summary = "Runs JavaScript in the page.",
    params = {
        code = { ty = "string", desc = "Script source." },
    },
)]
fn webview_run_js() {}

#[document(
    parent = "WebView",
    name = "texture",
    kind = "method",
    realm = "client",
    summary = "Surface texture id for the latest page image.",
    returns = { ty = "number", desc = "Texture id, or 0 after remove." },
)]
fn webview_texture() {}

#[document(
    parent = "WebView",
    name = "on_message",
    kind = "method",
    realm = "client",
    summary = "Sets the callback for window.engine.post. The callback receives one string.",
    params = {
        callback = { ty = "function", desc = "function(text)" },
    },
)]
fn webview_on_message() {}

#[document(
    parent = "WebView",
    name = "remove",
    kind = "method",
    realm = "client",
    summary = "Closes the page and frees its texture.",
)]
fn webview_remove() {}

pub fn register_webview_lib(
    lua: &Lua,
    bank: Arc<Mutex<Bank>>,
    pointer: Arc<Mutex<Pointer>>,
    queue: RenderQueue,
) {
    lua.set_app_data(WebApp {
        bank: bank.clone(),
        queue: queue.clone(),
        pointer,
    });
    let table = lua.create_table().expect("Failed to create webview table");
    table
        .set(
            "_listeners",
            lua.create_table().expect("Failed to create webview listeners"),
        )
        .expect("[webview] Failed setting listeners");
    table
        .set(
            "create",
            lua.create_function(move |lua, (width, height): (u32, u32)| {
                let Some(app) = lua.app_data_ref::<WebApp>() else {
                    return Ok(None);
                };
                let owned = Arc::clone(&app.bank);
                let queue = app.queue.clone();
                drop(app);
                let mut bank = owned.lock().expect("Couldn't lock webview");
                let Some((id, texture)) = bank.spawn(&queue, width, height) else {
                    return Ok(None);
                };
                drop(bank);

                Ok(Some(LuaView {
                    id,
                    texture,
                    bank: owned,
                    queue,
                    alive: true,
                }))
            })
            .expect("[webview] Failed to create create function"),
        )
        .expect("[webview] Failed setting create");
    lua.globals().set("webview", table).unwrap();
}
