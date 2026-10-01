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

    local out = resolved[class];

    if out == nil then
        out = {};
        resolved[class] = out;
    end

    if visiting[class] then
        print("[scripted_ents] inheritance cycle at " .. class);

        return out;
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
            print("[scripted_ents] " .. class .. " has missing base " .. tostring(base_name));
        else
            base_class = flatten(base_name, visiting);

            for key, value in pairs(base_class) do
                out[key] = value;
            end
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

function scripted_ents.register(ENT, class)
    if type(ENT) ~= "table" or type(class) ~= "string" then
        error("scripted_ents.register expects (table, string)", 2);
    end

    local previous = storage[class];

    if previous ~= nil and previous.base ~= nil and children[previous.base] ~= nil then
        children[previous.base][class] = nil;
    end

    storage[class] = ENT;
    by_hash[net.hash(class)] = class;

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

function scripted_ents.get_list()
    local out = {};

    for class in pairs(storage) do
        out[class] = scripted_ents.get(class);
    end

    return out;
end

function scripted_ents._resolve_all()
    local done = {};

    for class in pairs(storage) do
        if done[class] == nil then
            local visiting = {};
            local root = class;

            while storage[root] ~= nil and storage[root].base ~= nil and storage[storage[root].base] ~= nil and not visiting[root] do
                visiting[root] = true;
                root = storage[root].base;
            end

            flatten_tree(root, done);
        end
    end
end
