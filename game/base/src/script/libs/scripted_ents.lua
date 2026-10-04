--[=[document
kind = "library",
name = "scripted_ents",
realm = "shared",
summary = "Registers entity class tables and resolves inheritance. Every class except base_entity and base_weapon sets a base.",
]=]
scripted_ents = {};
scripted_ents._storage = scripted_ents._storage or {};
scripted_ents._resolved = scripted_ents._resolved or {};
scripted_ents._by_hash = scripted_ents._by_hash or {};
scripted_ents._children = scripted_ents._children or {};
scripted_ents._loading = false;
local storage = scripted_ents._storage;
local resolved = scripted_ents._resolved;
local by_hash = scripted_ents._by_hash;
local children = scripted_ents._children;

local function flatten(class, visiting)
    local raw = storage[class];

    if raw == nil then
        return nil;
    end

    if visiting[class] then
        error("scripted_ents.register: inheritance cycle at " .. class, 2);
    end

    local out = resolved[class];

    if out == nil then
        out = {};
    end

    visiting[class] = true;

    for key in pairs(out) do
        out[key] = nil;
    end

    for key, value in pairs(ents._meta) do
        out[key] = value;
    end

    local base_class = nil;
    local base_name = raw.base;

    if base_name ~= nil then
        if storage[base_name] == nil then
            error("scripted_ents.register: base '" .. tostring(base_name) .. "' is not registered", 2);
        end

        base_class = flatten(base_name, visiting);

        for key, value in pairs(base_class) do
            out[key] = value;
        end
    end

    for key, value in pairs(raw) do
        out[key] = value;
    end

    raw.class_name = class;
    raw.base_class = base_class;
    out.class_name = class;
    out.class_hash = net.hash(class);
    out.base_class = base_class;
    out.__index = out;
    visiting[class] = nil;
    resolved[class] = out;

    return out;
end

local function flatten_tree(class, done)
    if done[class] then
        return;
    end

    done[class] = true;
    flatten(class, {});
    local derived = children[class];

    if derived == nil then
        return;
    end

    for child in pairs(derived) do
        flatten_tree(child, done);
    end
end

--[=[document
parent = "scripted_ents",
name = "register",
realm = "shared",
summary = "Stores an entity class table. The class is flattened, including its base, when something asks for it.",
params = {
    ENT = { ty = "table", desc = "Class table. Set ENT.base or WEAPON.base to the parent class name." },
    class = { ty = "string", desc = "Class name. base_entity and base_weapon are roots and have no base." },
},
returns = { ty = "nil", desc = "" },
example = "scripted_ents.register({\n    base = \"base_entity\",\n    initialize = function(self) end,\n}, \"sent_box\")",
panics = "Errors if the arguments are wrong, the class is already registered, the name hash collides, or a base is missing.",
see_also = "ents.create, scripted_ents.get",
]=]
function scripted_ents.register(ENT, class)
    if type(ENT) ~= "table" or type(class) ~= "string" or class == "" then
        error("scripted_ents.register expects (table, string)", 2);
    end

    if storage[class] ~= nil then
        error("scripted_ents.register: '" .. class .. "' is already registered", 2);
    end

    local hash = net.hash(class);

    if hash == net.hash("Player") or by_hash[hash] ~= nil then
        error("scripted_ents.register: class hash for '" .. class .. "' collides", 2);
    end

    if class ~= "base_entity" and class ~= "base_weapon" then
        if type(ENT.base) ~= "string" or ENT.base == "" then
            error("scripted_ents.register: '" .. class .. "' is missing base", 2);
        end

        if not scripted_ents._loading and storage[ENT.base] == nil then
            error("scripted_ents.register: base '" .. ENT.base .. "' is not registered", 2);
        end
    end

    storage[class] = ENT;
    by_hash[hash] = class;

    if ENT.base ~= nil then
        local derived = children[ENT.base];

        if derived == nil then
            derived = {};
            children[ENT.base] = derived;
        end

        derived[class] = true;
    end

    if scripted_ents._loading then
        return;
    end

    flatten_tree(class, {});
end

--[=[document
parent = "scripted_ents",
name = "get_stored",
realm = "shared",
summary = "The class table as it was registered, without inherited keys.",
params = {
    class = { ty = "string", desc = "Class name." },
},
returns = { ty = "table", desc = "A table with def and base, or nil if the class is not registered." },
see_also = "scripted_ents.get",
]=]
function scripted_ents.get_stored(class)
    local raw = storage[class];

    if raw == nil then
        return nil;
    end

    return { def = raw, base = raw.base };
end

--[=[document
parent = "scripted_ents",
name = "is_based_on",
realm = "shared",
summary = "Walks the base chain and reports whether class derives from base.",
params = {
    class = { ty = "string", desc = "Class to test." },
    base = { ty = "string", desc = "Ancestor class, or the class itself." },
},
returns = { ty = "boolean", desc = "True if class is base or inherits from it." },
]=]
function scripted_ents.is_based_on(class, base)
    local current = class;

    while current ~= nil do
        if current == base then
            return true;
        end

        local raw = storage[current];

        if raw == nil then
            return false;
        end

        current = raw.base;
    end

    return false;
end

--[=[document
parent = "scripted_ents",
name = "get",
realm = "shared",
summary = "Returns the class table with inherited keys copied in from its bases.",
params = {
    class = { ty = "string", desc = "Class name." },
},
returns = { ty = "table", desc = "The flattened class, or nil if it is not registered." },
see_also = "scripted_ents.get_stored, scripted_ents.register",
]=]
function scripted_ents.get(class)
    local out = resolved[class];

    if out ~= nil then
        return out;
    end

    if storage[class] == nil then
        return nil;
    end

    return flatten(class, {});
end

--[=[document
parent = "scripted_ents",
name = "get_list",
realm = "shared",
summary = "Every registered class, flattened.",
returns = { ty = "table", desc = "Map of class name to the table from scripted_ents.get." },
see_also = "scripted_ents.get",
]=]
function scripted_ents.get_list()
    local out = {};

    for class in pairs(storage) do
        out[class] = scripted_ents.get(class);
    end

    return out;
end

local function base_problem(class)
    if class == "base_entity" or class == "base_weapon" then
        return nil;
    end

    local raw = storage[class];

    if raw == nil then
        return "scripted_ents.register: base '" .. class .. "' is not registered";
    end

    if type(raw.base) ~= "string" or raw.base == "" then
        return "scripted_ents.register: '" .. class .. "' is missing base";
    end

    if storage[raw.base] == nil then
        return "scripted_ents.register: base '" .. tostring(raw.base) .. "' is not registered";
    end

    return nil;
end

local function collect_broken()
    local bad = {};
    local skip = {};
    local known = {};

    local function walk(class, visiting)
        if known[class] then
            return skip[class] == true;
        end

        if visiting[class] then
            bad[class] = "scripted_ents.register: inheritance cycle at " .. class;
            skip[class] = true;
            known[class] = true;

            return true;
        end

        visiting[class] = true;
        local direct = base_problem(class);
        local broken = direct ~= nil;

        if direct ~= nil then
            bad[class] = direct;
        end

        local raw = storage[class];

        if not broken and raw ~= nil and type(raw.base) == "string" then
            broken = walk(raw.base, visiting);
        end

        if broken then
            skip[class] = true;
        end

        known[class] = true;
        visiting[class] = nil;

        return broken;
    end

    for class in pairs(storage) do
        walk(class, {});
    end

    return bad, skip;
end

function scripted_ents._resolve_all()
    local bad, skip = collect_broken();
    local done = {};

    local function flatten_ok(class)
        if done[class] or skip[class] then
            return;
        end

        done[class] = true;
        flatten(class, {});
        local derived = children[class];

        if derived == nil then
            return;
        end

        for child in pairs(derived) do
            flatten_ok(child);
        end
    end

    for class in pairs(storage) do
        if done[class] == nil and not skip[class] then
            local root = class;

            while storage[root] ~= nil and type(storage[root].base) == "string" and storage[storage[root].base] ~= nil and not skip[storage[root].base] do
                root = storage[root].base;
            end

            flatten_ok(root);
        end
    end

    local names = {};

    for class in pairs(bad) do
        names[#names + 1] = class;
    end

    table.sort(names);

    if #names == 0 then
        return;
    end

    local lines = {};

    for idx = 1, #names do
        lines[idx] = bad[names[idx]];
    end

    error(table.concat(lines, "\n"), 2);
end

scripted_ents.register({
    initialize = function() end,
    think = function() end,
    on_remove = function() end,
}, "base_entity");
