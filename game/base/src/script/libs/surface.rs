use crate::script::engine::{DrawCommand, RenderQueue};
use crate::ui::gfx::{self, Book};
use crate::ui::Color;
use mlua::Lua;
use r#macro::document;
//todo: stop using locks

#[document(
    kind = "library",
    name = "surface",
    realm = "client",
    summary = "Immediate drawing and gpu resources. Draw calls queue until the frame presents. Colors on draw_rect, draw_outlined_rect, and draw_text are 0 to 255. A non-zero id that is not live skips the draw."
)]
fn surface_lib() {}

#[document(
    parent = "surface",
    name = "draw_rect",
    kind = "function",
    realm = "client",
    summary = "Fills a rectangle.",
    params = {
        x = { ty = "number", desc = "Left edge in pixels." },
        y = { ty = "number", desc = "Top edge in pixels." },
        w = { ty = "number", desc = "Width in pixels." },
        h = { ty = "number", desc = "Height in pixels." },
        r = { ty = "number", desc = "Red, 0 to 255." },
        g = { ty = "number", desc = "Green, 0 to 255." },
        b = { ty = "number", desc = "Blue, 0 to 255." },
        a = { ty = "number", desc = "Alpha, 0 to 255." },
        texture = { ty = "number", desc = "Texture id. Omit it or pass 0 for none.", optional = true },
        pipeline = { ty = "number", desc = "Pipeline id. Omit it or pass 0 for the default.", optional = true },
        sampler = { ty = "number", desc = "Sampler id. Omit it or pass 0 for the default.", optional = true },
    },
    example = "surface.draw_rect(10, 10, 80, 24, 255, 255, 255, 255)",
)]
fn surface_draw_rect() {}

#[document(
    parent = "surface",
    name = "draw_outlined_rect",
    kind = "function",
    realm = "client",
    summary = "Strokes a rectangle.",
    params = {
        x = { ty = "number", desc = "Left edge in pixels." },
        y = { ty = "number", desc = "Top edge in pixels." },
        w = { ty = "number", desc = "Width in pixels." },
        h = { ty = "number", desc = "Height in pixels." },
        thickness = { ty = "number", desc = "Stroke width in pixels." },
        r = { ty = "number", desc = "Red, 0 to 255." },
        g = { ty = "number", desc = "Green, 0 to 255." },
        b = { ty = "number", desc = "Blue, 0 to 255." },
        a = { ty = "number", desc = "Alpha, 0 to 255." },
        texture = { ty = "number", desc = "Texture id. Omit it or pass 0 for none.", optional = true },
        pipeline = { ty = "number", desc = "Pipeline id. Omit it or pass 0 for the default.", optional = true },
        sampler = { ty = "number", desc = "Sampler id. Omit it or pass 0 for the default.", optional = true },
    },
)]
fn surface_draw_outlined_rect() {}

#[document(
    parent = "surface",
    name = "draw_text",
    kind = "function",
    realm = "client",
    summary = "Draws a string. Unknown font names use the built-in face.",
    params = {
        font = { ty = "string", desc = "Font name. default is the built-in face." },
        text = { ty = "string", desc = "The string." },
        x = { ty = "number", desc = "Left edge in pixels." },
        y = { ty = "number", desc = "Top edge in pixels." },
        scale = { ty = "number", desc = "Pixel height." },
        r = { ty = "number", desc = "Red, 0 to 255." },
        g = { ty = "number", desc = "Green, 0 to 255." },
        b = { ty = "number", desc = "Blue, 0 to 255." },
        a = { ty = "number", desc = "Alpha, 0 to 255." },
        texture = { ty = "number", desc = "Texture id. Omit it or pass 0 for none.", optional = true },
        pipeline = { ty = "number", desc = "Pipeline id. Omit it or pass 0 for the default.", optional = true },
        sampler = { ty = "number", desc = "Sampler id. Omit it or pass 0 for the default.", optional = true },
    },
    example = "surface.draw_text(\"default\", \"Hello\", 8, 8, 16, 255, 255, 255, 255)",
)]
fn surface_draw_text() {}

#[document(
    parent = "surface",
    name = "shader",
    kind = "function",
    realm = "client",
    summary = "Builtin shader id. Names are mesh, color, text, and skinned.",
    params = {
        name = { ty = "string", desc = "Builtin name." },
    },
    returns = { ty = "number", desc = "Shader id, or 0 when the name is unknown." },
)]
fn surface_shader() {}

#[document(
    parent = "surface",
    name = "pipeline",
    kind = "function",
    realm = "client",
    summary = "Builtin pipeline id. Names match surface.shader.",
    params = {
        name = { ty = "string", desc = "Builtin name." },
    },
    returns = { ty = "number", desc = "Pipeline id, or 0 when the name is unknown." },
    see_also = "surface.shader",
)]
fn surface_pipeline() {}

#[document(
    parent = "surface",
    name = "texture",
    kind = "function",
    realm = "client",
    summary = "Builtin texture id. Names are white and flat.",
    params = {
        name = { ty = "string", desc = "Builtin name." },
    },
    returns = { ty = "number", desc = "Texture id, or 0 when the name is unknown." },
)]
fn surface_texture() {}

#[document(
    parent = "surface",
    name = "sampler",
    kind = "function",
    realm = "client",
    summary = "Builtin sampler id. Names are wrap and clamp.",
    params = {
        name = { ty = "string", desc = "Builtin name." },
    },
    returns = { ty = "number", desc = "Sampler id, or 0 when the name is unknown." },
)]
fn surface_sampler() {}

#[document(
    parent = "surface",
    name = "create_shader",
    kind = "function",
    realm = "client",
    summary = "Creates a shader. A path that can be read is loaded as the source. Otherwise the string is WGSL.",
    params = {
        source = { ty = "string", desc = "File path or WGSL source." },
    },
    returns = { ty = "number", desc = "Shader id, or 0 when allocation fails." },
    see_also = "surface.create_pipeline, surface.free",
)]
fn surface_create_shader() {}

#[document(
    parent = "surface",
    name = "create_texture",
    kind = "function",
    realm = "client",
    summary = "Creates a texture from an image file.",
    params = {
        path = { ty = "string", desc = "Image path." },
    },
    returns = { ty = "number", desc = "Texture id, or 0 when allocation fails." },
    see_also = "surface.free",
)]
fn surface_create_texture() {}

#[document(
    parent = "surface",
    name = "material_texture",
    kind = "function",
    realm = "client",
    summary = "Creates a texture from a material name.",
    params = {
        name = { ty = "string", desc = "Material name." },
    },
    returns = { ty = "number", desc = "Texture id, or 0 when allocation fails." },
)]
fn surface_material_texture() {}

#[document(
    parent = "surface",
    name = "create_target",
    kind = "function",
    realm = "client",
    summary = "Creates a render target.",
    params = {
        width = { ty = "number", desc = "Width in pixels." },
        height = { ty = "number", desc = "Height in pixels." },
    },
    returns = { ty = "number", desc = "Target id, or 0 when allocation fails. The id can be drawn as a texture." },
    see_also = "surface.set_target, surface.free",
)]
fn surface_create_target() {}

#[document(
    parent = "surface",
    name = "create_buffer",
    kind = "function",
    realm = "client",
    summary = "Creates a buffer of little-endian floats.",
    params = {
        values = { ty = "table", desc = "Array of numbers." },
    },
    returns = { ty = "number", desc = "Buffer id, or 0 when allocation fails." },
    see_also = "surface.update_buffer, surface.free",
)]
fn surface_create_buffer() {}

#[document(
    parent = "surface",
    name = "create_sampler",
    kind = "function",
    realm = "client",
    summary = "Creates a sampler. nearest is point filtering. Any other filter, or none, is linear. repeat wraps. Any other wrap, or none, clamps.",
    params = {
        filter = { ty = "string", desc = "nearest or linear.", optional = true },
        wrap = { ty = "string", desc = "repeat or clamp.", optional = true },
    },
    returns = { ty = "number", desc = "Sampler id, or 0 when allocation fails." },
)]
fn surface_create_sampler() {}

#[document(
    parent = "surface",
    name = "create_pipeline",
    kind = "function",
    realm = "client",
    summary = "Creates a pipeline from a shader. screen uses the 8-float vertex. Any other layout, or none, uses the 9-float world vertex.",
    params = {
        shader = { ty = "number", desc = "Shader id. 0 is allowed." },
        layout = { ty = "string", desc = "screen for 2D meshes.", optional = true },
    },
    returns = { ty = "number", desc = "Pipeline id, or 0 when the shader id is dead or allocation fails." },
    see_also = "surface.create_shader, surface.create_mesh",
)]
fn surface_create_pipeline() {}

#[document(
    parent = "surface",
    name = "create_mesh",
    kind = "function",
    realm = "client",
    summary = "Creates a mesh. A screen vertex is x, y, u, v, r, g, b, a. A world vertex is x, y, z, u, v, r, g, b, a. Color components are shader values, usually 0 to 1. Extra floats past a full vertex are dropped.",
    params = {
        verts = { ty = "table", desc = "Packed vertex floats." },
        layout = { ty = "string", desc = "screen for 2D vertices. Omit it for world vertices.", optional = true },
    },
    returns = { ty = "number", desc = "Mesh id, or 0 when allocation fails." },
    see_also = "surface.draw_mesh, surface.free",
)]
fn surface_create_mesh() {}

#[document(
    parent = "surface",
    name = "free",
    kind = "function",
    realm = "client",
    summary = "Releases an id from a create function. Builtin ids and dead ids are left alone.",
    params = {
        id = { ty = "number", desc = "Resource id." },
    },
)]
fn surface_free() {}

#[document(
    parent = "surface",
    name = "draw_mesh",
    kind = "function",
    realm = "client",
    summary = "Draws a mesh.",
    params = {
        mesh = { ty = "number", desc = "Mesh id." },
        pipeline = { ty = "number", desc = "Pipeline id." },
        texture = { ty = "number", desc = "Texture id. Omit it or pass 0 for none.", optional = true },
        sampler = { ty = "number", desc = "Sampler id. Omit it or pass 0 for the default.", optional = true },
    },
    see_also = "surface.create_mesh",
)]
fn surface_draw_mesh() {}

#[document(
    parent = "surface",
    name = "set_target",
    kind = "function",
    realm = "client",
    summary = "Draws later commands into a render target. Omit the id, or pass 0, to draw to the window.",
    params = {
        id = { ty = "number", desc = "Target id.", optional = true },
    },
    see_also = "surface.create_target",
)]
fn surface_set_target() {}

#[document(
    parent = "surface",
    name = "update_buffer",
    kind = "function",
    realm = "client",
    summary = "Replaces the floats in a buffer. A dead id does nothing.",
    params = {
        id = { ty = "number", desc = "Buffer id." },
        values = { ty = "table", desc = "Array of numbers." },
    },
    see_also = "surface.create_buffer",
)]
fn surface_update_buffer() {}

#[document(
    parent = "surface",
    name = "update_mesh",
    kind = "function",
    realm = "client",
    summary = "Replaces the vertex floats in a mesh. A dead id does nothing.",
    params = {
        id = { ty = "number", desc = "Mesh id." },
        verts = { ty = "table", desc = "Packed vertex floats." },
    },
    see_also = "surface.create_mesh",
)]
fn surface_update_mesh() {}

#[document(
    parent = "surface",
    name = "update_texture",
    kind = "function",
    realm = "client",
    summary = "Reloads a texture from an image file. A dead id does nothing.",
    params = {
        id = { ty = "number", desc = "Texture id." },
        path = { ty = "string", desc = "Image path." },
    },
    see_also = "surface.create_texture",
)]
fn surface_update_texture() {}

#[document(
    parent = "surface",
    name = "update_target",
    kind = "function",
    realm = "client",
    summary = "Resizes a render target. A dead id does nothing.",
    params = {
        id = { ty = "number", desc = "Target id." },
        width = { ty = "number", desc = "Width in pixels." },
        height = { ty = "number", desc = "Height in pixels." },
    },
    see_also = "surface.create_target",
)]
fn surface_update_target() {}

#[document(
    parent = "surface",
    name = "set_scissor",
    kind = "function",
    realm = "client",
    summary = "Sets the scissor rect. Omit the arguments to clear it. surface.push_scissor is the stacked form.",
    params = {
        x = { ty = "number", desc = "Left edge in pixels.", optional = true },
        y = { ty = "number", desc = "Top edge in pixels.", optional = true },
        w = { ty = "number", desc = "Width in pixels.", optional = true },
        h = { ty = "number", desc = "Height in pixels.", optional = true },
    },
    see_also = "surface.push_scissor",
)]
fn surface_set_scissor() {}

#[document(
    parent = "surface",
    name = "size",
    kind = "function",
    realm = "client",
    summary = "Window size in pixels.",
    returns = { ty = "number", desc = "Width, then height. Both can be 0 before the first frame." },
)]
fn surface_size() {}

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
            lua
                .create_function(
                    move |_,
                          (mesh, pipeline, texture, sampler): (
                        u32,
                        u32,
                        Option<u32>,
                        Option<u32>,
                    )| {
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

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "set_scissor",
            lua.create_function(
                move |_, (x, y, w, h): (Option<f32>, Option<f32>, Option<f32>, Option<f32>)| {
                    let rect = match (x, y, w, h) {
                        (Some(x), Some(y), Some(w), Some(h)) => Some([x, y, w, h]),
                        _ => None,
                    };
                    render_queue_
                        .lock()
                        .expect("Couldn't lock render queue")
                        .commands
                        .push(DrawCommand::SetScissor { rect });

                    Ok(())
                },
            )
            .expect("[surface] Failed to create set_scissor function"),
        )
        .expect("[surface] Failed setting set_scissor function");

    let render_queue_ = render_queue.clone();
    surface_table
        .set(
            "size",
            lua.create_function(move |_, ()| {
                let state = render_queue_.lock().expect("Couldn't lock render queue");

                Ok((state.width, state.height))
            })
            .expect("[surface] Failed to create size function"),
        )
        .expect("[surface] Failed setting size function");

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
