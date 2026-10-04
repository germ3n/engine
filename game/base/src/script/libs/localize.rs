use mlua::{Lua, MultiValue, Value};
use r#macro::document;

#[document(
    kind = "library",
    name = "localize",
    realm = "shared",
    summary = "Looks up translated text. Strings live in lang/<language>.txt as Token = text lines, and are referenced as #Token. Arguments are written {:1}, {:2}, and so on, where 1 is the first argument. surface.draw_text resolves a #Token string without arguments on its own."
)]
fn localize_lib() {}

#[document(
    parent = "localize",
    name = "get",
    kind = "function",
    realm = "shared",
    summary = "Resolves a #Token and fills in its arguments. Text that does not start with # is returned unchanged. Unknown tokens, and placeholders with no argument, are left as written.",
    params = {
        text = { ty = "string", desc = "A token such as #Player_Joined." },
        args = { ty = "any", desc = "Values for {:1}, {:2}, ... Converted with tostring.", optional = true },
    },
    returns = { ty = "string", desc = "The localized text." },
    example = "localize.get(\"#Player_Score\", name, 12)",
)]
fn localize_get() {}

#[document(
    parent = "localize",
    name = "add",
    kind = "function",
    realm = "shared",
    summary = "Registers a custom string. It replaces a lang file string with the same token, survives set_language, and is used for its language before the english fallback. The text is used as written, so \\n is not an escape here.",
    params = {
        token = { ty = "string", desc = "Token name, with or without the leading #." },
        text = { ty = "string", desc = "The text. May contain {:1}, {:2}, ... placeholders." },
        language = { ty = "string", desc = "Language the string belongs to. Defaults to english.", optional = true },
    },
    example = "localize.add(\"#Round_Won\", \"{:1} won the round\")",
)]
fn localize_add() {}

#[document(
    parent = "localize",
    name = "set_language",
    kind = "function",
    realm = "shared",
    summary = "Switches to lang/<language>.txt. Tokens missing from it fall back to english.",
    params = {
        language = { ty = "string", desc = "Language name, such as english." },
    },
)]
fn localize_set_language() {}

#[document(
    parent = "localize",
    name = "language",
    kind = "function",
    realm = "shared",
    summary = "The current language name.",
    returns = { ty = "string", desc = "Language name." },
)]
fn localize_language() {}

pub fn register_localize_lib(lua: &Lua) {
    let table = lua.create_table().expect("Failed to create localize table");

    table
        .set(
            "get",
            lua.create_function(|lua, (text, rest): (mlua::LuaString, MultiValue)| {
                let text = text.to_string_lossy();
                let tostring: mlua::Function = lua.globals().get("tostring")?;
                let mut args = Vec::new();

                for value in rest {
                    let value = match value {
                        Value::String(text) => text.to_string_lossy().to_string(),
                        other => tostring.call::<mlua::LuaString>(other)?.to_string_lossy().to_string(),
                    };
                    args.push(value);
                }

                Ok(crate::localize::get(&text, &args))
            })
            .expect("[localize] Failed to create get"),
        )
        .expect("[localize] Failed setting get");

    table
        .set(
            "add",
            lua.create_function(
                |_, (token, text, language): (String, mlua::LuaString, Option<String>)| {
                    let language = language.unwrap_or_else(|| "english".to_string());
                    crate::localize::add(&language, &token, &text.to_string_lossy());

                    Ok(())
                },
            )
            .expect("[localize] Failed to create add"),
        )
        .expect("[localize] Failed setting add");

    table
        .set(
            "set_language",
            lua.create_function(|_, language: String| {
                crate::localize::set_language(&language);

                Ok(())
            })
            .expect("[localize] Failed to create set_language"),
        )
        .expect("[localize] Failed setting set_language");

    table
        .set(
            "language",
            lua.create_function(|_, ()| Ok(crate::localize::language()))
                .expect("[localize] Failed to create language"),
        )
        .expect("[localize] Failed setting language");

    lua.globals()
        .set("localize", table)
        .expect("Failed to set localize table");
}
