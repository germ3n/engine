use crate::script::{pick_scripts, run_file, Realm};
use mlua::{Function, Lua, Table, Value};
use std::collections::BTreeMap;

const ENTITIES_PREFIX: &str = "lua/entities/";

pub fn register_scripted_ents_lib(lua: &Lua) {
    crate::script::exec(lua, "scripted_ents.lua", "lua/libs/scripted_ents.luac");
}

fn pick(files: &[String], stem: &str) -> Option<String> {
    let compiled = format!("/{stem}.luac");
    let source = format!("/{stem}.lua");

    files
        .iter()
        .find(|name| name.ends_with(&compiled) || name.ends_with(&source))
        .cloned()
}

fn load_class(lua: &Lua, class: &str, files: &[String], realm: Realm) -> Result<(), String> {
    let globals = lua.globals();
    let ent = lua.create_table().map_err(|err| err.to_string())?;
    globals.set("ENT", ent).map_err(|err| err.to_string())?;

    let realm_stem = match realm {
        Realm::Server => "init",
        Realm::Client => "cl_init",
        Realm::Menu => return Ok(()),
    };

    for stem in ["shared", realm_stem] {
        if let Some(path) = pick(files, stem) {
            run_file(lua, &path)?;
        }
    }

    let ent: Value = globals.get("ENT").map_err(|err| err.to_string())?;
    globals.set("ENT", Value::Nil).map_err(|err| err.to_string())?;

    let Value::Table(ent) = ent else {
        return Err("ENT is not a table".to_string());
    };

    let scripted_ents: Table = globals.get("scripted_ents").map_err(|err| err.to_string())?;
    let register: Function = scripted_ents.get("register").map_err(|err| err.to_string())?;

    register.call::<()>((ent, class)).map_err(|err| err.to_string())
}

pub fn load_entities(lua: &Lua, realm: Realm) {
    let Some(fs) = crate::fs::try_global() else {
        return;
    };

    let mut classes: BTreeMap<String, Vec<String>> = BTreeMap::new();

    for path in fs.list_prefix(ENTITIES_PREFIX) {
        let Some(rest) = path.strip_prefix(ENTITIES_PREFIX) else {
            continue;
        };

        let mut parts = rest.split('/');
        let (Some(class), Some(_file), None) = (parts.next(), parts.next(), parts.next()) else {
            continue;
        };

        classes.entry(class.to_string()).or_default().push(path.clone());
    }

    let scripted_ents: Table = lua
        .globals()
        .get("scripted_ents")
        .expect("Failed to get scripted_ents");
    scripted_ents
        .set("_loading", true)
        .expect("[scripted_ents] Failed setting _loading");

    for (class, files) in &classes {
        let files = pick_scripts(&fs, files);

        match load_class(lua, class, &files, realm) {
            Ok(()) => log::info!("[scripted_ents] loaded {class}"),
            Err(err) => log::error!("[scripted_ents] {class}: {err}"),
        }

        let _ = lua.globals().set("ENT", Value::Nil);
    }

    scripted_ents
        .set("_loading", false)
        .expect("[scripted_ents] Failed setting _loading");
    let resolve_all: Function = scripted_ents
        .get("_resolve_all")
        .expect("Failed to get scripted_ents._resolve_all");

    if let Err(err) = resolve_all.call::<()>(()) {
        log::error!("[scripted_ents] resolve failed: {err}");
    }
}

#[cfg(test)]
mod tests {
    use crate::console::{ConVar, ConVarValue};
    use crate::entities::{EntityList, ScriptedEntity};
    use crate::network::events::NetVar;
    use crate::script::libs::ents::{self, EntityScope};
    use crate::script::{Realm, ScriptEngine};
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    fn boot_fs() {
        if crate::fs::try_global().is_some() {
            return;
        }

        let fs = crate::fs::Fs::boot().expect("fs boot");
        crate::fs::set_global(Arc::new(fs));
    }

    fn engine(realm: Realm) -> ScriptEngine {
        boot_fs();
        let mut cvars = HashMap::new();
        cvars.insert(
            "sv_gravity".to_string(),
            Arc::new(ConVar::new(
                "sv_gravity",
                ConVarValue::Float(24.0),
                "World gravity",
                Some(false),
                Some(true),
            )),
        );
        let binds = Arc::new(Mutex::new(crate::input::Binds::defaults()));
        let pads = Arc::new(Mutex::new(crate::platform::PadCache::new()));

        ScriptEngine::new(
            realm,
            1.0 / 60.0,
            Arc::new(cvars),
            binds,
            pads,
            std::ptr::null_mut(),
        )
    }

    fn exec(engine: &ScriptEngine, source: &str) {
        engine
            .lua
            .load(source)
            .set_name("test.lua")
            .exec()
            .unwrap_or_else(|err| panic!("{err}"));
    }

    fn assert_hook_log(log: &str) {
        assert!(log.contains(":initialize error:"), "{log}");
        assert!(log.contains("init failed"), "{log}");
        assert!(log.contains(":on_spawn error:"), "{log}");
        assert!(log.contains("spawn failed"), "{log}");
    }

    #[test]
    fn child_can_register_before_its_base() {
        let engine = engine(Realm::Server);
        exec(
            &engine,
            r#"
            local function check(cond, message)
                if not cond then
                    error(message, 2);
                end
            end

            scripted_ents._resolve_all();

            scripted_ents._loading = true;
            scripted_ents.register({
                base = "sent_zzz_parent",
                rank = 3,
            }, "sent_aaa_child");
            check(scripted_ents.get_stored("sent_aaa_child").base == "sent_zzz_parent", "stored before parent");
            scripted_ents.register({
                base = "sent_mmm_mid",
                rank = 2,
                ping = function()
                    return "mid";
                end,
            }, "sent_zzz_parent");
            scripted_ents.register({
                base = "base_entity",
                rank = 1,
                tag = "low",
                ping = function()
                    return "low";
                end,
            }, "sent_mmm_mid");
            scripted_ents._loading = false;
            scripted_ents._resolve_all();

            local child = scripted_ents.get("sent_aaa_child");
            local parent = scripted_ents.get("sent_zzz_parent");
            local mid = scripted_ents.get("sent_mmm_mid");
            check(child.rank == 3, "child rank");
            check(child.tag == "low", "inherited tag");
            check(child.ping() == "mid", "inherited ping");
            check(child.base_class == parent, "child base_class");
            check(parent.base_class == mid, "parent base_class");
            check(scripted_ents.is_based_on("sent_aaa_child", "sent_mmm_mid"), "based on mid");
            check(scripted_ents.is_based_on("sent_aaa_child", "base_entity"), "based on base_entity");

            scripted_ents.register({
                base = "sent_mmm_mid",
                rank = 4,
            }, "sent_after_order");
            local after = scripted_ents.get("sent_after_order");
            check(after.rank == 4, "runtime child rank");
            check(after.ping() == "low", "runtime child ping");

            scripted_ents._loading = true;
            local ok_missing = pcall(scripted_ents.register, {}, "sent_nobase_order");
            scripted_ents._loading = false;
            check(not ok_missing, "missing base should fail during load");

            local ok_late, err_late = pcall(scripted_ents.register, { base = "sent_not_loaded" }, "sent_late_missing");
            check(not ok_late, "late register should fail");
            check(string.find(err_late, "not registered", 1, true) ~= nil, tostring(err_late));

            scripted_ents._loading = true;
            scripted_ents.register({ base = "sent_no_such" }, "sent_orphan_order");
            scripted_ents.register({ base = "sent_orphan_order" }, "sent_orphan_child");
            scripted_ents.register({
                base = "base_entity",
                rank = 11,
            }, "sent_good_beside_orphan");
            scripted_ents._loading = false;
            local ok_res, err_res = pcall(scripted_ents._resolve_all);
            check(not ok_res, "orphan resolve should fail");
            check(string.find(tostring(err_res), "sent_no_such", 1, true) ~= nil, tostring(err_res));
            check(scripted_ents.get("sent_good_beside_orphan").rank == 11, "good class beside orphan");
            check(scripted_ents.get_stored("sent_orphan_child") ~= nil, "orphan child stayed registered");
            check(not pcall(scripted_ents.get, "sent_orphan_child"), "orphan child should not resolve");
            check(type(scripted_ents.get("base_entity").think) == "function", "base think intact");
            check(scripted_ents.get("sent_blaster").clip_size == 10, "blaster intact");

            scripted_ents._loading = true;
            scripted_ents.register({ base = "sent_cycle_b" }, "sent_cycle_a");
            scripted_ents.register({ base = "sent_cycle_a" }, "sent_cycle_b");
            scripted_ents.register({ base = "sent_cycle_self" }, "sent_cycle_self");
            scripted_ents._loading = false;
            local ok_cycle, err_cycle = pcall(scripted_ents._resolve_all);
            check(not ok_cycle, "cycle resolve should fail");
            check(string.find(tostring(err_cycle), "inheritance cycle", 1, true) ~= nil, tostring(err_cycle));
            check(not pcall(scripted_ents.get, "sent_cycle_a"), "cycle should not resolve");
            check(type(scripted_ents.get("base_entity").think) == "function", "base think intact after cycle");
            check(scripted_ents.get("sent_aaa_child").rank == 3, "earlier child intact");
            "#,
        );
    }

    #[test]
    fn class_tables_are_copied_onto_each_instance() {
        let engine = engine(Realm::Server);
        let mut list = EntityList::new();
        let _scope = EntityScope::new(&engine.entity_access, &mut list);
        exec(
            &engine,
            r#"
            local function check(cond, message)
                if not cond then
                    error(message, 2);
                end
            end

            local shared = { n = 1 };
            local loop = { n = 4 };
            loop.self = loop;

            scripted_ents.register({
                base = "base_entity",
                stats = {
                    hp = 10,
                    tags = { "a" },
                    ping = function(self)
                        return self.hp;
                    end,
                },
                clip_size = 3,
            }, "sent_copy_parent");
            scripted_ents.register({
                base = "sent_copy_parent",
                left = shared,
                right = shared,
                loop = loop,
            }, "sent_copy_child");

            local class_child = scripted_ents.get("sent_copy_child");
            local class_parent = scripted_ents.get("sent_copy_parent");
            local a = ents.create("sent_copy_child");
            local b = ents.create("sent_copy_child");
            local parent = ents.create("sent_copy_parent");

            check(a.stats ~= b.stats, "instances share stats");
            check(a.stats ~= class_child.stats, "instance shares class stats");
            check(a.stats ~= parent.stats, "child instance shares parent instance stats");
            check(a.stats.tags ~= b.stats.tags, "instances share nested table");
            check(a.stats.tags ~= class_child.stats.tags, "instance shares class nested table");
            check(a.left == a.right, "aliased fields diverged");
            check(a.left ~= b.left, "instances share aliased table");
            check(a.left ~= class_child.left, "instance shares class aliased table");
            check(a.loop.self == a.loop, "cycle was broken");
            check(a.loop ~= class_child.loop, "instance shares class cycle");
            check(a.base_class == class_parent, "base_class was copied");
            check(b.base_class == class_parent, "second base_class was copied");
            check(a.stats.ping == class_parent.stats.ping, "function was copied");

            a.stats.hp = 1;
            a.stats.tags[1] = "z";
            b.stats.hp = 2;
            parent.stats.hp = 4;
            a.clip_size = 9;
            a.left.n = 8;
            a.loop.n = 6;

            check(a.stats.hp == 1, "instance a stats");
            check(b.stats.hp == 2, "instance b stats");
            check(parent.stats.hp == 4, "parent instance stats");
            check(class_child.stats.hp == 10, "child class stats changed");
            check(class_parent.stats.hp == 10, "parent class stats changed");
            check(a.stats.tags[1] == "z", "instance a tags");
            check(b.stats.tags[1] == "a", "instance b tags");
            check(class_child.stats.tags[1] == "a", "class tags changed");
            check(a.clip_size == 9, "instance a clip");
            check(b.clip_size == 3, "instance b clip");
            check(a.stats.ping(a.stats) == 1, "ping sees the instance table");
            check(a.right.n == 8, "alias was split");
            check(b.left.n == 1, "other instance alias changed");
            check(class_child.left.n == 1, "class alias changed");
            check(a.loop.n == 6, "cycle copy");
            check(class_child.loop.n == 4, "class cycle changed");
            check(class_child.loop.self == class_child.loop, "class cycle");
            "#,
        );
    }

    const HOOK_CREATE: &str = r##"
        local hits = { init = 0, spawn = 0 };

        scripted_ents.register({
            base = "base_entity",
            initialize = function()
                hits.init = hits.init + 1;
            end,
            on_spawn = function()
                hits.spawn = hits.spawn + 1;
            end,
        }, "sent_ok_hooks");

        local ok_ent = ents.create("sent_ok_hooks");
        ok_ent:spawn();

        if hits.init ~= 1 or hits.spawn ~= 1 then
            error("expected hooks to run, init=" .. hits.init .. " spawn=" .. hits.spawn);
        end

        test_lines = {};
        print = function(...)
            local parts = {};
            local count = select("#", ...);

            for idx = 1, count do
                parts[idx] = tostring(select(idx, ...));
            end

            test_lines[#test_lines + 1] = table.concat(parts, "\t");
        end

        scripted_ents.register({
            base = "base_entity",
            initialize = function(self)
                self.ready = true;
                error("init failed");
            end,
            on_spawn = function(self)
                self.saw = true;
                error("spawn failed");
            end,
            think = function(self)
                self.thought = true;
            end,
        }, "sent_fail_hooks");

        test_ent = ents.create("sent_fail_hooks");
        test_ent:spawn();
    "##;

    fn read_fail(engine: &ScriptEngine, name: &str) -> (bool, bool, bool, bool, String) {
        engine
            .lua
            .load(&format!(
                r#"
                local ent = {name}
                return ent.ready == true, ent.saw == true, ent.thought == true, ent._spawned == true, table.concat(test_lines, "\n")
                "#
            ))
            .set_name("read.lua")
            .eval()
            .unwrap_or_else(|err| panic!("{err}"))
    }

    fn assert_failed_hooks(ready: bool, saw: bool, thought: bool, spawned: bool, log: &str) {
        assert!(ready, "{log}");
        assert!(saw, "{log}");
        assert!(thought, "{log}");
        assert!(spawned, "{log}");
        assert_hook_log(log);
    }

    fn create_failing(engine: &ScriptEngine) -> (bool, bool, bool, bool, String) {
        let mut list = EntityList::new();
        let _scope = EntityScope::new(&engine.entity_access, &mut list);
        exec(engine, HOOK_CREATE);
        engine.think_entities(1.0, 0.016, 1);

        read_fail(engine, "test_ent")
    }

    #[test]
    fn hook_errors_are_reported_on_server_and_client() {
        let server = engine(Realm::Server);
        let (ready, saw, thought, spawned, server_log) = create_failing(&server);
        assert_failed_hooks(ready, saw, thought, spawned, &server_log);

        let net = engine(Realm::Client);
        exec(
            &net,
            r##"
            test_lines = {};
            print = function(...)
                local parts = {};
                local count = select("#", ...);

                for idx = 1, count do
                    parts[idx] = tostring(select(idx, ...));
                end

                test_lines[#test_lines + 1] = table.concat(parts, "\t");
            end

            scripted_ents.register({
                base = "base_entity",
                initialize = function(self)
                    self.ready = true;
                    error("init failed");
                end,
                on_spawn = function(self)
                    self.saw = true;
                    error("spawn failed");
                end,
                think = function(self)
                    self.thought = true;
                end,
            }, "sent_fail_net");
            "##,
        );
        let hash: f64 = net
            .lua
            .load("return scripted_ents.get('sent_fail_net').class_hash")
            .eval()
            .unwrap();
        let mut list = EntityList::new();
        let mut entity = ScriptedEntity::new(hash as u32);
        entity.spawned = true;
        let handle = list.spawn(Box::new(entity)).unwrap();
        let index = handle.index();
        let _scope = EntityScope::new(&net.entity_access, &mut list);
        let vars: &[NetVar] = &[];
        let spawned_net = ents::net_spawn(&net.lua, handle, vars, None).expect("net_spawn");
        assert!(spawned_net);
        net.think_entities(1.0, 0.016, 1);
        let (ready, saw, thought, spawned, net_log) =
            read_fail(&net, &format!("ents.get_by_index({index})"));
        assert_failed_hooks(ready, saw, thought, spawned, &net_log);
    }
}
