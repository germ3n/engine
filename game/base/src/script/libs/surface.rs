use crate::script::engine::{DrawCommand, RenderQueue};
use crate::ui::gfx::{self, Book};
use crate::ui::Color;
use mlua::Lua;
//todo: stop using locks

pub fn register_surface_lib(lua: &Lua, render_queue: RenderQueue) {
    let surface_table = lua.create_table().unwrap();

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "draw_rect",
            lua.create_function(
                move |_,
                      (x, y, w, h, r, g, b, a, texture, pipeline, sampler): (
                    f32,
                    f32,
                    f32,
                    f32,
                    f32,
                    f32,
                    f32,
                    f32,
                    Option<u32>,
                    Option<u32>,
                    Option<u32>,
                )| {
                    let texture = texture.unwrap_or(0);
                    let pipeline = pipeline.unwrap_or(0);
                    let sampler = sampler.unwrap_or(0);

                    if !ids_live(&render_queue_, &[texture, pipeline, sampler]) {
                        return Ok(());
                    }

                    render_queue_
                        .lock()
                        .expect("Couldn't lock render queue")
                        .commands
                        .push(DrawCommand::Rect {
                            x,
                            y,
                            w,
                            h,
                            color: Color::ColorRGBAf {
                                r: r / 255.0,
                                g: g / 255.0,
                                b: b / 255.0,
                                a: a / 255.0,
                            },
                            texture,
                            pipeline,
                            sampler,
                        });

                    Ok(())
                },
            )
            .expect("[surface] Failed to create draw_rect function"),
        )
        .expect("[surface] Failed setting draw_rect function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "draw_outlined_rect",
            lua.create_function(
                move |_,
                      (x, y, w, h, thickness, r, g, b, a, texture, pipeline, sampler): (
                    f32,
                    f32,
                    f32,
                    f32,
                    f32,
                    f32,
                    f32,
                    f32,
                    f32,
                    Option<u32>,
                    Option<u32>,
                    Option<u32>,
                )| {
                    let texture = texture.unwrap_or(0);
                    let pipeline = pipeline.unwrap_or(0);
                    let sampler = sampler.unwrap_or(0);

                    if !ids_live(&render_queue_, &[texture, pipeline, sampler]) {
                        return Ok(());
                    }

                    render_queue_
                        .lock()
                        .expect("Couldn't lock render queue")
                        .commands
                        .push(DrawCommand::OutlinedRect {
                            x,
                            y,
                            w,
                            h,
                            thickness,
                            color: Color::ColorRGBAf {
                                r: r / 255.0,
                                g: g / 255.0,
                                b: b / 255.0,
                                a: a / 255.0,
                            },
                            texture,
                            pipeline,
                            sampler,
                        });

                    Ok(())
                },
            )
            .expect("[surface] Failed to create draw_outlined_rect function"),
        )
        .expect("[surface] Failed setting draw_outlined_rect function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "draw_text",
            lua.create_function(
                move |_,
                      (font, text, x, y, scale, r, g, b, a, texture, pipeline, sampler): (
                    mlua::LuaString,
                    mlua::LuaString,
                    f32,
                    f32,
                    f32,
                    f32,
                    f32,
                    f32,
                    f32,
                    Option<u32>,
                    Option<u32>,
                    Option<u32>,
                )| {
                    let texture = texture.unwrap_or(0);
                    let pipeline = pipeline.unwrap_or(0);
                    let sampler = sampler.unwrap_or(0);

                    if !ids_live(&render_queue_, &[texture, pipeline, sampler]) {
                        return Ok(());
                    }

                    render_queue_
                        .lock()
                        .expect("Couldn't lock render queue")
                        .commands
                        .push(DrawCommand::Text {
                            font,
                            text,
                            x,
                            y,
                            scale,
                            color: Color::ColorRGBAf {
                                r: r / 255.0,
                                g: g / 255.0,
                                b: b / 255.0,
                                a: a / 255.0,
                            },
                            texture,
                            pipeline,
                            sampler,
                        });

                    Ok(())
                },
            )
            .expect("[surface] Failed to create draw_text function"),
        )
        .expect("[surface] Failed setting draw_text function");

    surface_table
        .set(
            "shader",
            lua.create_function(|_, name: String| Ok(gfx::builtin_shader(&name)))
                .expect("[surface] Failed to create shader function"),
        )
        .expect("[surface] Failed setting shader function");

    surface_table
        .set(
            "pipeline",
            lua.create_function(|_, name: String| Ok(gfx::builtin_pipeline(&name)))
                .expect("[surface] Failed to create pipeline function"),
        )
        .expect("[surface] Failed setting pipeline function");

    surface_table
        .set(
            "texture",
            lua.create_function(|_, name: String| Ok(gfx::builtin_texture(&name)))
                .expect("[surface] Failed to create texture function"),
        )
        .expect("[surface] Failed setting texture function");

    surface_table
        .set(
            "sampler",
            lua.create_function(|_, name: String| Ok(gfx::builtin_sampler(&name)))
                .expect("[surface] Failed to create sampler function"),
        )
        .expect("[surface] Failed setting sampler function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "create_shader",
            lua.create_function(move |_, source: String| {
                let source = shader_text(&source);

                Ok(queue_create(&render_queue_, gfx::KIND_SHADER, |id| {
                    DrawCommand::CreateShader { id, source }
                }))
            })
            .expect("[surface] Failed to create create_shader function"),
        )
        .expect("[surface] Failed setting create_shader function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "create_texture",
            lua.create_function(move |_, path: String| {
                Ok(queue_create(&render_queue_, gfx::KIND_TEXTURE, |id| {
                    DrawCommand::CreateTexture { id, path }
                }))
            })
            .expect("[surface] Failed to create create_texture function"),
        )
        .expect("[surface] Failed setting create_texture function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "material_texture",
            lua.create_function(move |_, name: String| {
                Ok(queue_create(&render_queue_, gfx::KIND_TEXTURE, |id| {
                    DrawCommand::CreateMaterial { id, name }
                }))
            })
            .expect("[surface] Failed to create material_texture function"),
        )
        .expect("[surface] Failed setting material_texture function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "create_target",
            lua.create_function(move |_, (width, height): (u32, u32)| {
                Ok(queue_create(&render_queue_, gfx::KIND_TARGET, |id| {
                    DrawCommand::CreateTarget { id, width, height }
                }))
            })
            .expect("[surface] Failed to create create_target function"),
        )
        .expect("[surface] Failed setting create_target function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "create_buffer",
            lua.create_function(move |_, values: Vec<f32>| {
                let bytes = f32_bytes(&values);

                Ok(queue_create(&render_queue_, gfx::KIND_BUFFER, |id| {
                    DrawCommand::CreateBuffer { id, bytes }
                }))
            })
            .expect("[surface] Failed to create create_buffer function"),
        )
        .expect("[surface] Failed setting create_buffer function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "create_sampler",
            lua.create_function(move |_, (filter, wrap): (Option<String>, Option<String>)| {
                let linear = filter
                    .as_deref()
                    .map(|value| !value.eq_ignore_ascii_case("nearest"))
                    .unwrap_or(true);
                let repeat = wrap
                    .as_deref()
                    .map(|value| value.eq_ignore_ascii_case("repeat"))
                    .unwrap_or(false);

                Ok(queue_create(&render_queue_, gfx::KIND_SAMPLER, |id| {
                    DrawCommand::CreateSampler { id, linear, repeat }
                }))
            })
            .expect("[surface] Failed to create create_sampler function"),
        )
        .expect("[surface] Failed setting create_sampler function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "create_pipeline",
            lua.create_function(move |_, (shader, layout): (u32, Option<String>)| {
                let screen = layout
                    .as_deref()
                    .map(|value| value.eq_ignore_ascii_case("screen"))
                    .unwrap_or(false);

                if shader != 0 && !book_live(&render_queue_, shader) {
                    return Ok(0);
                }

                Ok(queue_create(&render_queue_, gfx::KIND_PIPELINE, |id| {
                    DrawCommand::CreatePipeline { id, shader, screen }
                }))
            })
            .expect("[surface] Failed to create create_pipeline function"),
        )
        .expect("[surface] Failed setting create_pipeline function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "create_mesh",
            lua.create_function(move |_, (verts, layout): (Vec<f32>, Option<String>)| {
                let screen = layout
                    .as_deref()
                    .map(|value| value.eq_ignore_ascii_case("screen"))
                    .unwrap_or(false);
                let stride = if screen {
                    gfx::SCREEN_FLOATS
                } else {
                    gfx::MESH_FLOATS
                };
                let count = verts.len() / stride;
                let verts = verts[..count * stride].to_vec();

                Ok(queue_create(&render_queue_, gfx::KIND_MESH, |id| {
                    DrawCommand::CreateMesh { id, verts, screen }
                }))
            })
            .expect("[surface] Failed to create create_mesh function"),
        )
        .expect("[surface] Failed setting create_mesh function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "free",
            lua.create_function(move |_, id: u32| {
                let mut state = render_queue_.lock().expect("Couldn't lock render queue");

                if !state.book.doom(id) {
                    return Ok(());
                }

                state.commands.push(DrawCommand::Free { id });

                Ok(())
            })
            .expect("[surface] Failed to create free function"),
        )
        .expect("[surface] Failed setting free function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "draw_mesh",
            lua.create_function(
                move |_, (mesh, pipeline, texture, sampler): (u32, u32, Option<u32>, Option<u32>)| {
                    let texture = texture.unwrap_or(0);
                    let sampler = sampler.unwrap_or(0);

                    if !ids_live(&render_queue_, &[mesh, pipeline, texture, sampler]) {
                        return Ok(());
                    }

                    render_queue_
                        .lock()
                        .expect("Couldn't lock render queue")
                        .commands
                        .push(DrawCommand::DrawMesh {
                            mesh,
                            pipeline,
                            texture,
                            sampler,
                        });

                    Ok(())
                },
            )
            .expect("[surface] Failed to create draw_mesh function"),
        )
            .expect("[surface] Failed setting draw_mesh function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "set_target",
            lua.create_function(move |_, id: Option<u32>| {
                let id = id.unwrap_or(0);

                if id != 0 && !book_live(&render_queue_, id) {
                    return Ok(());
                }

                render_queue_
                    .lock()
                    .expect("Couldn't lock render queue")
                    .commands
                    .push(DrawCommand::SetTarget { id });

                Ok(())
            })
            .expect("[surface] Failed to create set_target function"),
        )
        .expect("[surface] Failed setting set_target function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "update_buffer",
            lua.create_function(move |_, (id, values): (u32, Vec<f32>)| {
                if !book_live(&render_queue_, id) {
                    return Ok(());
                }

                render_queue_
                    .lock()
                    .expect("Couldn't lock render queue")
                    .commands
                    .push(DrawCommand::UpdateBuffer {
                        id,
                        bytes: f32_bytes(&values),
                    });

                Ok(())
            })
            .expect("[surface] Failed to create update_buffer function"),
        )
        .expect("[surface] Failed setting update_buffer function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "update_mesh",
            lua.create_function(move |_, (id, verts): (u32, Vec<f32>)| {
                if !book_live(&render_queue_, id) {
                    return Ok(());
                }

                render_queue_
                    .lock()
                    .expect("Couldn't lock render queue")
                    .commands
                    .push(DrawCommand::UpdateMesh { id, verts });

                Ok(())
            })
            .expect("[surface] Failed to create update_mesh function"),
        )
        .expect("[surface] Failed setting update_mesh function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "update_texture",
            lua.create_function(move |_, (id, path): (u32, String)| {
                if !book_live(&render_queue_, id) {
                    return Ok(());
                }

                render_queue_
                    .lock()
                    .expect("Couldn't lock render queue")
                    .commands
                    .push(DrawCommand::UpdateTexture { id, path });

                Ok(())
            })
            .expect("[surface] Failed to create update_texture function"),
        )
        .expect("[surface] Failed setting update_texture function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "update_target",
            lua.create_function(move |_, (id, width, height): (u32, u32, u32)| {
                if !book_live(&render_queue_, id) {
                    return Ok(());
                }

                render_queue_
                    .lock()
                    .expect("Couldn't lock render queue")
                    .commands
                    .push(DrawCommand::UpdateTarget { id, width, height });

                Ok(())
            })
            .expect("[surface] Failed to create update_target function"),
        )
        .expect("[surface] Failed setting update_target function");

    lua.globals().set("surface", surface_table).unwrap();
}

fn f32_bytes(values: &[f32]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(values.len() * 4);
    let mut idx = 0;

    while idx < values.len() {
        bytes.extend_from_slice(&values[idx].to_le_bytes());
        idx += 1;
    }

    bytes
}

fn shader_text(text: &str) -> String {
    match crate::fs::read_string(text) {
        Ok(source) => source,
        Err(_) => text.to_string(),
    }
}

fn queue_create(queue: &RenderQueue, kind: u32, command: impl FnOnce(u32) -> DrawCommand) -> u32 {
    let mut state = queue.lock().expect("Couldn't lock render queue");
    let id = state.book.alloc(kind);

    if id == 0 {
        return 0;
    }

    state.commands.push(command(id));

    id
}

fn book_live(queue: &RenderQueue, id: u32) -> bool {
    queue
        .lock()
        .expect("Couldn't lock render queue")
        .book
        .live(id)
}

fn ids_live(queue: &RenderQueue, ids: &[u32]) -> bool {
    let book = &queue.lock().expect("Couldn't lock render queue").book;
    let mut idx = 0;

    while idx < ids.len() {
        if ids[idx] != 0 && !live(book, ids[idx]) {
            return false;
        }

        idx += 1;
    }

    true
}

fn live(book: &Book, id: u32) -> bool {
    book.live(id)
}
