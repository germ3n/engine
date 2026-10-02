use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

mod parse;
mod html;

pub use parse::{parse_document, parse_lua_source, parse_rust_source};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind
{
    Library,
    Class,
    Function,
    Method,
    Hook,
    Field,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Realm
{
    Client,
    Server,
    Shared,
    Menu,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang
{
    Rust,
    Lua,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Access
{
    Public,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Param
{
    pub name: String,
    pub ty: String,
    pub desc: String,
    pub optional: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Ret
{
    pub ty: String,
    pub desc: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Page
{
    pub id: String,
    pub parent: String,
    pub name: String,
    pub kind: Kind,
    pub realm: Realm,
    pub summary: String,
    pub syntax: String,
    pub params: Vec<Param>,
    pub returns: Vec<Ret>,
    pub example: Option<String>,
    pub note: Option<String>,
    pub panics: Option<String>,
    pub safety: Option<String>,
    pub since: Option<String>,
    pub see_also: Vec<String>,
    pub deprecated: Option<String>,
    pub deprecated_since: Option<String>,
    pub unimplemented: Option<String>,
    pub access: Access,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lang: Option<Lang>,
    #[serde(skip)]
    pub origin: String,
}

#[derive(Clone, Debug, Default)]
pub struct RawDoc
{
    pub parent: Option<String>,
    pub name: Option<String>,
    pub kind: Option<Kind>,
    pub realm: Option<Realm>,
    pub summary: Option<String>,
    pub params: Vec<Param>,
    pub returns: Vec<Ret>,
    pub example: Option<String>,
    pub note: Option<String>,
    pub panics: Option<String>,
    pub safety: Option<String>,
    pub since: Option<String>,
    pub see_also: Vec<String>,
    pub deprecated: Option<String>,
    pub deprecated_since: Option<String>,
    pub unimplemented: Option<Option<String>>,
    pub access: Option<Access>,
}

pub fn interpret(raw: RawDoc, fallback_name: Option<&str>) -> Result<Page, String>
{
    let summary = match raw.summary
    {
        Some(summary) if !summary.is_empty() => summary,
        _ =>
        {
            return Err("missing summary".to_string());
        }
    };
    let name = match raw.name
    {
        Some(name) if !name.is_empty() => name,
        _ => match fallback_name
        {
            Some(name) if !name.is_empty() => name.to_string(),
            _ =>
            {
                return Err("missing name".to_string());
            }
        },
    };
    let mut parent = raw.parent.unwrap_or_default();
    let kind = match raw.kind
    {
        Some(kind) => kind,
        None =>
        {
            if parent.chars().next().is_some_and(|ch| ch.is_uppercase())
            {
                Kind::Method
            }
            else
            {
                Kind::Function
            }
        }
    };

    if matches!(kind, Kind::Library | Kind::Class)
    {
        parent.clear();
    }
    else if parent.is_empty() && !matches!(kind, Kind::Function)
    {
        return Err("missing parent".to_string());
    }

    let realm = raw.realm.unwrap_or(Realm::Shared);
    let access = raw.access.unwrap_or(Access::Public);
    let example = nonempty(raw.example);
    let note = nonempty(raw.note);
    let panics = nonempty(raw.panics);
    let safety = nonempty(raw.safety);
    let since = nonempty(raw.since);
    let deprecated = nonempty(raw.deprecated);
    let deprecated_since = nonempty(raw.deprecated_since);
    let unimplemented = match raw.unimplemented
    {
        None => None,
        Some(None) => Some("Not implemented.".to_string()),
        Some(Some(text)) if text.is_empty() => Some("Not implemented.".to_string()),
        Some(Some(text)) => Some(text),
    };
    let syntax = syntax(kind, &parent, &name, &raw.params, &raw.returns);
    let id = page_id(kind, &parent, &name);

    Ok(Page {
        id,
        parent,
        name,
        kind,
        realm,
        summary,
        syntax,
        params: raw.params,
        returns: raw.returns,
        example,
        note,
        panics,
        safety,
        since,
        see_also: raw.see_also,
        deprecated,
        deprecated_since,
        unimplemented,
        access,
        lang: None,
        origin: String::new(),
    })
}

fn nonempty(value: Option<String>) -> Option<String>
{
    match value
    {
        Some(text) if !text.is_empty() => Some(text),
        _ => None,
    }
}

fn page_id(kind: Kind, parent: &str, name: &str) -> String
{
    match kind
    {
        Kind::Library | Kind::Class => name.to_string(),
        Kind::Method | Kind::Hook => format!("{parent}:{name}"),
        Kind::Function | Kind::Field =>
        {
            if parent.is_empty()
            {
                return name.to_string();
            }

            format!("{parent}.{name}")
        }
    }
}

fn syntax(kind: Kind, parent: &str, name: &str, params: &[Param], returns: &[Ret]) -> String
{
    match kind
    {
        Kind::Field =>
        {
            let ty = returns
                .first()
                .map(|item| item.ty.as_str())
                .filter(|ty| !ty.is_empty())
                .unwrap_or("");
            let head = if parent.is_empty()
            {
                name.to_string()
            }
            else
            {
                format!("{parent}.{name}")
            };

            if ty.is_empty()
            {
                return head;
            }

            format!("{ty} {head}")
        }
        Kind::Library => name.to_string(),
        Kind::Class =>
        {
            if params.is_empty()
            {
                return name.to_string();
            }

            format!("{name}({})", arg_list(params))
        }
        Kind::Method | Kind::Hook => format!("{parent}:{name}({})", arg_list(params)),
        Kind::Function =>
        {
            let head = if parent.is_empty()
            {
                name.to_string()
            }
            else
            {
                format!("{parent}.{name}")
            };

            format!("{head}({})", arg_list(params))
        }
    }
}

fn arg_list(params: &[Param]) -> String
{
    let mut parts = Vec::new();

    for param in params
    {
        let mut piece = String::new();

        if !param.ty.is_empty()
        {
            piece.push_str(&param.ty);
            piece.push(' ');
        }

        piece.push_str(&param.name);

        if param.optional
        {
            piece.push_str(" = nil");
        }

        parts.push(piece);
    }

    parts.join(", ")
}

pub fn assemble(mut pages: Vec<Page>) -> Result<Vec<Page>, String>
{
    let mut seen: HashMap<String, String> = HashMap::new();
    let mut errors = Vec::new();

    for page in &pages
    {
        if let Some(previous) = seen.insert(page.id.clone(), page.origin.clone())
        {
            errors.push(format!(
                "duplicate {}\n  {previous}\n  {}",
                page.id, page.origin
            ));
        }
    }

    if !errors.is_empty()
    {
        return Err(errors.join("\n"));
    }

    let mut extra = Vec::new();
    let mut considered = HashSet::new();

    for page in &pages
    {
        if page.parent.is_empty() || !considered.insert(page.parent.clone())
        {
            continue;
        }

        if seen.contains_key(&page.parent)
        {
            continue;
        }

        extra.push(synthetic(&page.parent, parent_kind(&pages, &page.parent)));
    }

    pages.extend(extra);
    pages.sort_by(|left, right| left.id.cmp(&right.id));

    Ok(pages)
}

fn parent_kind(pages: &[Page], parent: &str) -> Kind
{
    let class = pages.iter().any(|page| {
        page.parent == parent && matches!(page.kind, Kind::Method | Kind::Hook | Kind::Field)
    });

    if class
    {
        return Kind::Class;
    }

    Kind::Library
}

fn synthetic(name: &str, kind: Kind) -> Page
{
    Page {
        id: name.to_string(),
        parent: String::new(),
        name: name.to_string(),
        kind,
        realm: Realm::Shared,
        summary: format!("Reference for {name}."),
        syntax: name.to_string(),
        params: Vec::new(),
        returns: Vec::new(),
        example: None,
        note: None,
        panics: None,
        safety: None,
        since: None,
        see_also: Vec::new(),
        deprecated: None,
        deprecated_since: None,
        unimplemented: None,
        access: Access::Public,
        lang: None,
        origin: "<generated>".to_string(),
    }
}

fn lang_of(origin: &str) -> Option<Lang>
{
    let path = match origin.rsplit_once(':')
    {
        Some((path, line)) if !line.is_empty() && line.bytes().all(|byte| byte.is_ascii_digit()) => path,
        _ => origin,
    };

    if path.ends_with(".rs")
    {
        return Some(Lang::Rust);
    }

    if path.ends_with(".lua")
    {
        return Some(Lang::Lua);
    }

    None
}

pub fn scan(root: &Path) -> Result<Vec<Page>, String>
{
    let mut pages = Vec::new();
    walk(root, &mut pages)?;

    assemble(pages)
}

fn walk(dir: &Path, pages: &mut Vec<Page>) -> Result<(), String>
{
    let listing = fs::read_dir(dir).map_err(|err| format!("failed to read {}: {err}", dir.display()))?;
    let mut entries: Vec<_> = listing.collect::<Result<Vec<_>, _>>().map_err(|err| err.to_string())?;
    entries.sort_by_key(|entry| entry.path());

    for entry in entries
    {
        let path = entry.path();

        if path.is_dir()
        {
            walk(&path, pages)?;

            continue;
        }

        let Some(ext) = path.extension().and_then(|ext| ext.to_str()) else
        {
            continue;
        };

        if ext != "rs" && ext != "lua"
        {
            continue;
        }

        let source = fs::read_to_string(&path)
            .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
        let label = path.display().to_string();
        let found = if ext == "rs"
        {
            parse_rust_source(&label, &source)?
        }
        else
        {
            parse_lua_source(&label, &source)?
        };
        pages.extend(found);
    }

    Ok(())
}

pub fn render(pages: &[Page]) -> String
{
    html::render(pages)
}

pub fn generate(src: &Path, dest: &Path) -> Result<(), String>
{
    let pages = scan(src)?;
    let html = render(&pages);

    if let Some(parent) = dest.parent()
    {
        fs::create_dir_all(parent).map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }

    fs::write(dest, html).map_err(|err| format!("failed to write {}: {err}", dest.display()))?;

    Ok(())
}

#[cfg(test)]
mod tests
{
    use super::*;

    const CREATE: &str = r#"
--[=[document
parent = "ents",
name = "create",
realm = "server",
summary = "Creates a scripted entity.",
params = {
    class = { ty = "string", desc = "Class name." },
},
returns = { ty = "Entity", desc = "The new entity." },
example = "local ent = ents.create(\"sent_blaster\")",
]=]
"#;

    #[test]
    fn parses_rust_attribute()
    {
        let source = r#"
#[document(
    parent = "ents",
    name = "create",
    realm = "server",
    summary = "Creates a scripted entity.",
    params = {
        class = { ty = "string", desc = "Class name." },
    },
    returns = { ty = "Entity", desc = "The new entity." },
    example = "local ent = ents.create(\"sent_blaster\")",
)]
fn ents_create() {}
"#;
        let pages = parse_rust_source("api.rs", source).unwrap();

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].id, "ents.create");
        assert_eq!(pages[0].realm, Realm::Server);
        assert_eq!(pages[0].params[0].name, "class");
        assert_eq!(pages[0].params[0].ty, "string");
        assert_eq!(pages[0].returns[0].ty, "Entity");
        assert_eq!(pages[0].syntax, "ents.create(string class)");
        assert!(pages[0].origin.starts_with("api.rs:"));
        assert_eq!(pages[0].lang, Some(Lang::Rust));
    }

    #[test]
    fn parses_lua_block()
    {
        let pages = parse_lua_source("ents.lua", CREATE).unwrap();

        assert_eq!(pages.len(), 1);
        assert_eq!(pages[0].id, "ents.create");
        assert_eq!(pages[0].kind, Kind::Function);
        assert_eq!(pages[0].example.as_deref(), Some("local ent = ents.create(\"sent_blaster\")"));
        assert!(pages[0].origin.contains("ents.lua:"));
        assert_eq!(pages[0].lang, Some(Lang::Lua));
        assert_eq!(pages[0].access, Access::Public);
    }

    #[test]
    fn rejects_missing_summary()
    {
        let source = "--[=[document\nparent = \"ents\",\nname = \"create\",\n]=]\n";
        let err = parse_lua_source("a.lua", source).unwrap_err();

        assert!(err.contains("a.lua:1"), "{err}");
        assert!(err.contains("missing summary"), "{err}");
    }

    #[test]
    fn parses_internal_flag()
    {
        let source = r#"
#[document(
    parent = "net",
    name = "call",
    summary = "Runs a callback.",
    internal,
)]
fn net_call() {}
"#;
        let pages = parse_rust_source("api.rs", source).unwrap();

        assert_eq!(pages[0].access, Access::Internal);
    }

    #[test]
    fn rejects_duplicate_id()
    {
        let first = parse_lua_source("a.lua", CREATE).unwrap();
        let second = parse_lua_source("b.lua", CREATE).unwrap();
        let mut pages = first;
        pages.extend(second);
        let err = assemble(pages).unwrap_err();

        assert!(err.contains("duplicate"), "{err}");
        assert!(err.contains("ents.create"), "{err}");
    }

    #[test]
    fn render_contains_signature_realm_and_example()
    {
        let pages = assemble(parse_lua_source("sample.lua", CREATE).unwrap()).unwrap();
        let html = render(&pages);

        assert!(html.contains("ents.create(string class)"), "{html}");
        assert!(html.contains("\"realm\":\"server\""), "{html}");
        assert!(html.contains("\"lang\":\"lua\""), "{html}");
        assert!(html.contains("\"access\":\"public\""), "{html}");
        assert!(html.contains("sent_blaster"), "{html}");
    }

    #[test]
    fn scan_base_src_has_slice()
    {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../base/src");
        let pages = scan(&root).unwrap();
        let ids: HashSet<&str> = pages.iter().map(|page| page.id.as_str()).collect();
        let expected = [
            "ents",
            "Entity",
            "Vector3",
            "Angle3",
            "hook",
            "net",
            "scripted_ents",
            "engine",
            "ents.create",
            "ents.get_by_index",
            "ents.get_all",
            "ents.get",
            "ents.remove",
            "ents.get_by_class",
            "ents.find_by_class",
            "ents.get_count",
            "Entity:index",
            "Entity:handle",
            "Entity:get_class",
            "Entity:is_valid",
            "Entity:spawn",
            "Entity:remove",
            "Entity:get_pos",
            "Entity:set_pos",
            "Entity:get_angles",
            "Entity:set_angles",
            "Entity:get_velocity",
            "Entity:set_velocity",
            "Entity:set_next_think",
            "Entity:get_networked",
            "Entity:set_networked",
            "Entity:set_owner",
            "Entity:get_owner",
            "Entity:set_interpolated",
            "Entity:set_model",
            "Entity:set_sequence",
            "Entity:play_gesture",
            "Entity:stop_gesture",
            "Entity:initialize",
            "Entity:on_spawn",
            "Entity:think",
            "Entity:predicted_think",
            "Entity:on_remove",
            "Vector3.x",
            "Vector3.y",
            "Vector3.z",
            "Vector3:add_inplace",
            "Vector3:sub_inplace",
            "Vector3:mul_inplace",
            "Vector3:dot",
            "Vector3:cross",
            "Vector3:len_sq",
            "Vector3:len",
            "Vector3:normalize",
            "Vector3:normalize_inplace",
            "Vector3.sum_all",
            "Angle3.p",
            "Angle3.y",
            "Angle3.r",
            "Angle3:add_inplace",
            "Angle3:sub_inplace",
            "Angle3:mul_inplace",
            "Angle3:normalize",
            "Angle3:normalize_inplace",
            "Angle3.sum_all",
            "hook.add",
            "hook.call",
            "net.add_callback",
            "net.call",
            "net.hash_to_name",
            "net.hash",
            "net.writer",
            "net.send",
            "scripted_ents.register",
            "scripted_ents.get_stored",
            "scripted_ents.is_based_on",
            "scripted_ents.get",
            "scripted_ents.get_list",
            "engine.tick_interval",
            "engine.curtime",
            "engine.frametime",
            "engine.tick_count",
            "engine.first_time_predicted",
        ];

        for id in expected
        {
            assert!(ids.contains(id), "missing {id}");
        }

        let create = pages.iter().find(|page| page.id == "ents.create").unwrap();
        let send = pages.iter().find(|page| page.id == "net.send").unwrap();

        assert_eq!(create.lang, Some(Lang::Lua));
        assert_eq!(send.lang, Some(Lang::Rust));
    }
}
