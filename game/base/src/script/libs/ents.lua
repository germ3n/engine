local ffi = require("ffi");

local vector_type = ffi.typeof("Vector3");
local angle_type = ffi.typeof("Angle3");
local floor = math.floor;
local pcall = pcall;
local next = next;
local type = type;
local rawequal = rawequal;

local INDEX_SPAN = 2097152;
local INT_MIN = -2147483648;
local INT_MAX = 2147483647;
local TAG_NIL = 0;
local TAG_BOOL = 1;
local TAG_INT = 2;
local TAG_FLOAT = 3;
local TAG_STRING = 4;
local TAG_VECTOR3 = 5;
local TAG_ANGLE3 = 6;
local TAG_ENTITY = 7;
local VAR_STRIDE = 5;

return function(native)
    ents = {};
    ents._storage = ents._storage or {};
    ents._meta = ents._meta or {};
    ents._think = ents._think or {};
    ents._dirty = ents._dirty or {};
    local storage = ents._storage;
    local meta = ents._meta;
    local think_list = ents._think;
    local dirty = ents._dirty;
    local native_meta = { __index = meta };
    local native_classes = native.classes;
    local all = {};
    local all_revision = -1;
    local think_count = 0;
    local thinking = false;
    local think_holes = false;
    local local_raw = 0;
    local saved = {};
    local SAVED_NIL = {};

    local native_create = native.create;
    local native_spawn = native.spawn;
    local native_remove = native.remove;
    local native_class_hash = native.class_hash;
    local native_is_spawned = native.is_spawned;
    local native_raw_at = native.raw_at;
    local native_count = native.count;
    local native_revision = native.revision;
    local native_handles = native.handles;
    local native_handle = native.handle;
    local native_get_pos = native.get_pos;
    local native_set_pos = native.set_pos;
    local native_get_angles = native.get_angles;
    local native_set_angles = native.set_angles;
    local native_get_velocity = native.get_velocity;
    local native_set_velocity = native.set_velocity;
    local native_set_owner = native.set_owner;
    local native_get_owner = native.get_owner;
    local attach_owned;

    local function report(ent, name, err)
        print("[ents] " .. tostring(ent._class) .. ":" .. name .. " error: " .. tostring(err));
    end

    local function wrap(raw)
        local index = raw % INDEX_SPAN;
        local ent = storage[index];

        if ent ~= nil and ent._handle == raw then
            return ent;
        end

        local class_hash = native_class_hash(raw);

        if class_hash == nil then
            return nil;
        end

        local class = scripted_ents._by_hash[class_hash];
        local mt = native_meta;
        local spawned = true;

        if class ~= nil then
            mt = scripted_ents.get(class);
            spawned = native_is_spawned(raw);
        else
            class = native_classes[class_hash] or "unknown";
        end

        ent = setmetatable({
            _handle = raw,
            _index = index,
            _class = class,
            _networked = {},
            _next_think = 0,
            _think_idx = 0,
            _spawned = spawned,
            _removed = false,
            _owner = 0,
        }, mt);
        storage[index] = ent;

        local owner_raw = native_get_owner(raw);

        if owner_raw ~= nil then
            attach_owned(ent, owner_raw);
        end

        return ent;
    end

    local function sort_owned(a, b)
        return a._handle < b._handle;
    end

    local function detach_owned(ent)
        local owner_raw = ent._owner;

        if owner_raw == 0 then
            return;
        end

        ent._owner = 0;
        local owner = storage[owner_raw % INDEX_SPAN];

        if owner == nil or owner._handle ~= owner_raw or owner._owned == nil then
            return;
        end

        local owned = owner._owned;

        for idx = 1, #owned do
            if owned[idx] == ent then
                table.remove(owned, idx);

                return;
            end
        end
    end

    attach_owned = function(ent, owner_raw)
        detach_owned(ent);

        if owner_raw == 0 then
            return;
        end

        ent._owner = owner_raw;
        local owner = wrap(owner_raw);

        if owner == nil then
            return;
        end

        local owned = owner._owned;

        if owned == nil then
            owned = {};
            owner._owned = owned;
        end

        owned[#owned + 1] = ent;
        table.sort(owned, sort_owned);
    end

    local function add_think(ent)
        if ent._think_idx ~= 0 then
            return;
        end

        think_count = think_count + 1;
        think_list[think_count] = ent;
        ent._think_idx = think_count;
    end

    local function remove_think(ent)
        local idx = ent._think_idx;

        if idx == 0 then
            return;
        end

        ent._think_idx = 0;

        if thinking then
            think_list[idx] = false;
            think_holes = true;

            return;
        end

        local moved = think_list[think_count];
        think_list[think_count] = nil;
        think_count = think_count - 1;

        if idx <= think_count then
            think_list[idx] = moved;
            moved._think_idx = idx;
        end
    end

    local function compact_think()
        local count = 0;

        for idx = 1, think_count do
            local ent = think_list[idx];

            if ent then
                count = count + 1;
                think_list[count] = ent;
                ent._think_idx = count;
            end
        end

        for idx = count + 1, think_count do
            think_list[idx] = nil;
        end

        think_count = count;
        think_holes = false;
    end

    local function unlink(ent)
        ent._removed = true;
        remove_think(ent);
        detach_owned(ent);
        dirty[ent] = nil;
        saved[ent] = nil;

        if storage[ent._index] == ent then
            storage[ent._index] = nil;
        end
    end

    local function start(ent)
        if type(ent.think) == "function" then
            add_think(ent);
        end

        local on_spawn = ent.on_spawn;

        if on_spawn ~= nil then
            on_spawn(ent);
        end
    end

    local function encode(out, n, key, value)
        local kind = type(value);
        out[n] = key;

        if rawequal(value, nil) then
            out[n + 1] = TAG_NIL;
        elseif kind == "boolean" then
            out[n + 1] = TAG_BOOL;
            out[n + 2] = value;
        elseif kind == "number" then
            if value == floor(value) and value >= INT_MIN and value <= INT_MAX then
                out[n + 1] = TAG_INT;
            else
                out[n + 1] = TAG_FLOAT;
            end

            out[n + 2] = value;
        elseif kind == "string" then
            out[n + 1] = TAG_STRING;
            out[n + 2] = value;
        elseif kind == "table" then
            out[n + 1] = TAG_ENTITY;
            out[n + 2] = value._handle;
        elseif ffi.istype(vector_type, value) then
            out[n + 1] = TAG_VECTOR3;
            out[n + 2] = value.x;
            out[n + 3] = value.y;
            out[n + 4] = value.z;
        else
            out[n + 1] = TAG_ANGLE3;
            out[n + 2] = value.p;
            out[n + 3] = value.y;
            out[n + 4] = value.r;
        end

        return n + VAR_STRIDE;
    end

    local function decode(flat, at)
        local tag = flat[at + 1];

        if tag == TAG_VECTOR3 then
            return vector_type(flat[at + 2], flat[at + 3], flat[at + 4]);
        elseif tag == TAG_ANGLE3 then
            return angle_type(flat[at + 2], flat[at + 3], flat[at + 4]);
        elseif tag == TAG_ENTITY then
            return wrap(flat[at + 2]);
        elseif tag == TAG_NIL then
            return nil;
        end

        return flat[at + 2];
    end

    local function check_value(value)
        local kind = type(value);

        if kind == "nil" or kind == "boolean" or kind == "number" or kind == "string" then
            return true;
        end

        if kind == "table" then
            return type(value._handle) == "number";
        end

        if kind == "cdata" then
            return ffi.istype(vector_type, value) or ffi.istype(angle_type, value);
        end

        return false;
    end

    local function same(old, value)
        local kind = type(value);

        if type(old) ~= kind then
            return false;
        end

        if kind ~= "cdata" then
            return old == value;
        end

        if ffi.istype(vector_type, value) then
            return ffi.istype(vector_type, old) and old.x == value.x and old.y == value.y and old.z == value.z;
        end

        return ffi.istype(angle_type, old) and old.p == value.p and old.y == value.y and old.r == value.r;
    end

    local function encode_entity(out, n, ent)
        local networked = ent._networked;

        if next(networked) == nil then
            return n;
        end

        out[n] = ent._handle;
        local count_at = n + 1;
        local count = 0;
        n = n + 2;

        for key, value in pairs(networked) do
            n = encode(out, n, key, value);
            count = count + 1;
        end

        out[count_at] = count;

        return n;
    end

    local function notify_changed(ent, key, old, value)
        local callback = ent.on_networked_changed;

        if callback == nil then
            return;
        end

        local ok, err = pcall(callback, ent, key, old, value);

        if not ok then
            report(ent, "on_networked_changed", err);
        end
    end

    local function apply_vars(ent, flat, at, count, notify)
        local networked = ent._networked;
        local predicted = nil;
        local skipped = 0;

        if local_raw ~= 0 and (ent._handle == local_raw or ent._owner == local_raw) then
            predicted = ent._predicted;
        end

        for _ = 1, count do
            local key = flat[at];

            if predicted == nil or not predicted[key] then
                local value = decode(flat, at);
                local old = networked[key];
                networked[key] = value;

                if notify and not same(old, value) then
                    notify_changed(ent, key, old, value);
                end
            else
                skipped = skipped + 1;
            end

            at = at + VAR_STRIDE;
        end

        return at, skipped;
    end

    local function encode_predicted(out, n, ent)
        local predicted = ent._predicted;

        if predicted == nil then
            return n;
        end

        local networked = ent._networked;
        out[n] = ent._handle;
        local count_at = n + 1;
        local count = 0;
        n = n + 2;

        for key in pairs(predicted) do
            n = encode(out, n, key, networked[key]);
            count = count + 1;
        end

        out[count_at] = count;

        return n;
    end

    local function run_predicted(ent, cmd)
        local predicted_think = ent.predicted_think;

        if predicted_think == nil then
            return;
        end

        local ok, err = pcall(predicted_think, ent, cmd);

        if not ok then
            report(ent, "predicted_think", err);
        end
    end

    function meta:index()
        return self._index;
    end

    function meta:handle()
        return native_handle(self._handle);
    end

    function meta:get_class()
        return self._class;
    end

    function meta:is_valid()
        return not self._removed;
    end

    function meta:spawn()
        if self._spawned or self._removed then
            return;
        end

        if not native_spawn(self._handle) then
            return;
        end

        self._spawned = true;
        start(self);
    end

    function meta:remove()
        if self._removed then
            return;
        end

        native_remove(self._handle);
        unlink(self);
    end

    function meta:get_pos()
        return vector_type(native_get_pos(self._handle));
    end

    function meta:set_pos(pos)
        native_set_pos(self._handle, pos.x, pos.y, pos.z);
    end

    function meta:get_angles()
        return angle_type(native_get_angles(self._handle));
    end

    function meta:set_angles(angles)
        native_set_angles(self._handle, angles.p, angles.y, angles.r);
    end

    function meta:get_velocity()
        return vector_type(native_get_velocity(self._handle));
    end

    function meta:set_velocity(velocity)
        native_set_velocity(self._handle, velocity.x, velocity.y, velocity.z);
    end

    function meta:set_next_think(time)
        self._next_think = time;
    end

    function meta:get_networked(key, fallback)
        local value = self._networked[key];

        if rawequal(value, nil) then
            return fallback;
        end

        return value;
    end

    function meta:set_owner(owner)
        if CLIENT then
            error("set_owner is server only", 2);
        end

        local owner_raw = 0;

        if owner ~= nil then
            owner_raw = owner._handle;
        end

        if not native_set_owner(self._handle, owner_raw) then
            return;
        end

        attach_owned(self, owner_raw);
    end

    function meta:get_owner()
        if self._owner == 0 then
            return nil;
        end

        return wrap(self._owner);
    end

    function meta:set_networked(key, value, predicted)
        if type(key) ~= "string" then
            error("networked key must be a string", 2);
        end

        if not check_value(value) then
            error("unsupported networked type " .. type(value) .. " for " .. key, 2);
        end

        if predicted then
            local keys = self._predicted;

            if keys == nil then
                keys = {};
                self._predicted = keys;
            end

            keys[key] = true;
        end

        local networked = self._networked;

        if CLIENT then
            networked[key] = value;

            return;
        end

        if same(networked[key], value) then
            return;
        end

        networked[key] = value;

        if not self._spawned or self._removed then
            return;
        end

        local keys = dirty[self];

        if keys == nil then
            keys = {};
            dirty[self] = keys;
        end

        keys[key] = true;
    end

    function ents.create(class)
        if CLIENT then
            error("ents.create is server only", 2);
        end

        local resolved = scripted_ents.get(class);

        if resolved == nil then
            error("unknown entity class " .. tostring(class), 2);
        end

        if native_classes[resolved.class_hash] ~= nil then
            error("cannot create native class " .. class, 2);
        end

        local raw = native_create(resolved.class_hash);

        if raw == nil then
            return nil;
        end

        local ent = wrap(raw);
        local initialize = ent.initialize;

        if initialize ~= nil then
            initialize(ent);
        end

        return ent;
    end

    function ents.get_by_index(index)
        local ent = storage[index];

        if ent ~= nil then
            return ent;
        end

        local raw = native_raw_at(index);

        if raw == nil then
            return nil;
        end

        return wrap(raw);
    end

    function ents.get_all()
        local revision = native_revision();

        if revision == all_revision then
            return all;
        end

        local handles = native_handles();
        local out = {};
        local count = 0;

        for idx = 1, #handles do
            local ent = wrap(handles[idx]);

            if ent ~= nil then
                count = count + 1;
                out[count] = ent;
            end
        end

        all = out;
        all_revision = revision;

        return out;
    end

    function ents.find_by_class(class)
        local list = ents.get_all();
        local out = {};
        local count = 0;

        for idx = 1, #list do
            local ent = list[idx];

            if ent._class == class then
                count = count + 1;
                out[count] = ent;
            end
        end

        return out;
    end

    function ents.get_count()
        return native_count();
    end

    local exports = {};

    function exports.think(cur_time)
        local count = think_count;
        thinking = true;

        for idx = 1, count do
            local ent = think_list[idx];

            if ent and ent._next_think <= cur_time then
                local ok, err = pcall(ent.think, ent);

                if not ok then
                    report(ent, "think", err);
                end
            end
        end

        thinking = false;

        if think_holes then
            compact_think();
        end
    end

    function exports.removed(list, count)
        for idx = 1, count do
            local raw = list[idx];
            local ent = storage[raw % INDEX_SPAN];

            if ent ~= nil and ent._handle == raw then
                unlink(ent);
            end
        end
    end

    function exports.net_spawn(raw, flat, count, len)
        local class_hash = native_class_hash(raw);

        if class_hash == nil or scripted_ents._by_hash[class_hash] == nil then
            return false;
        end

        local ent = wrap(raw);
        ent._spawned = true;
        apply_vars(ent, flat, 1, count, false);

        local initialize = ent.initialize;

        if initialize ~= nil then
            local ok, err = pcall(initialize, ent);

            if not ok then
                report(ent, "initialize", err);
            end
        end

        local ok, err = pcall(start, ent);

        if not ok then
            report(ent, "on_spawn", err);
        end

        return true;
    end

    function exports.collect_networked()
        if next(dirty) == nil then
            return nil, nil;
        end

        local out = {};
        local n = 1;

        for ent, keys in pairs(dirty) do
            local networked = ent._networked;
            out[n] = ent._handle;
            local count_at = n + 1;
            local count = 0;
            n = n + 2;

            for key in pairs(keys) do
                n = encode(out, n, key, networked[key]);
                count = count + 1;
            end

            out[count_at] = count;
        end

        for ent in pairs(dirty) do
            dirty[ent] = nil;
        end

        return out, n;
    end

    function exports.networked_state(raw)
        local out = {};
        local n = 1;

        if raw ~= nil then
            local ent = storage[raw % INDEX_SPAN];

            if ent == nil or ent._handle ~= raw then
                return nil, nil;
            end

            n = encode_entity(out, n, ent);
        else
            for _, ent in pairs(storage) do
                if ent._spawned then
                    n = encode_entity(out, n, ent);
                end
            end
        end

        if n == 1 then
            return nil, nil;
        end

        return out, n;
    end

    function exports.apply_networked(flat, len)
        local at = 1;
        local skipped = 0;
        local missing = 0;

        while at < len do
            local raw = flat[at];
            local count = flat[at + 1];
            at = at + 2;
            local ent = wrap(raw);

            if ent ~= nil then
                local ent_skipped;
                at, ent_skipped = apply_vars(ent, flat, at, count, true);
                skipped = skipped + ent_skipped;
            else
                at = at + count * VAR_STRIDE;
                missing = missing + 1;
            end
        end

        return skipped, missing;
    end

    function exports.predicted(raw, tick, buttons, wish_x, wish_y, wish_z, view_p, view_y, view_r, first_time)
        local ent = wrap(raw);

        if ent == nil then
            return;
        end

        local cmd = {
            tick = tick,
            buttons = buttons,
            wish = vector_type(wish_x, wish_y, wish_z),
            view = angle_type(view_p, view_y, view_r),
        };
        engine.first_time_predicted = first_time;
        run_predicted(ent, cmd);
        local owned = ent._owned;

        if owned ~= nil then
            for idx = 1, #owned do
                local child = owned[idx];

                if child ~= nil then
                    run_predicted(child, cmd);
                end
            end
        end

        engine.first_time_predicted = true;
    end

    function exports.predicted_state(raw)
        local ent = wrap(raw);

        if ent == nil then
            return nil, nil;
        end

        local out = {};
        local n = encode_predicted(out, 1, ent);
        local owned = ent._owned;

        if owned ~= nil then
            for idx = 1, #owned do
                n = encode_predicted(out, n, owned[idx]);
            end
        end

        if n == 1 then
            return nil, nil;
        end

        return out, n;
    end

    function exports.begin_reconcile(flat, len)
        saved = {};
        local at = 1;

        while at < len do
            local raw = flat[at];
            local count = flat[at + 1];
            at = at + 2;
            local ent = wrap(raw);

            if ent == nil then
                at = at + count * VAR_STRIDE;
            else
                local networked = ent._networked;
                local predicted = ent._predicted;

                if predicted == nil then
                    predicted = {};
                    ent._predicted = predicted;
                end

                local keys = saved[ent];

                if keys == nil then
                    keys = {};
                    saved[ent] = keys;
                end

                for _ = 1, count do
                    local key = flat[at];
                    predicted[key] = true;

                    if rawequal(keys[key], nil) then
                        local old = networked[key];

                        if rawequal(old, nil) then
                            old = SAVED_NIL;
                        end

                        keys[key] = old;
                    end

                    networked[key] = decode(flat, at);
                    at = at + VAR_STRIDE;
                end
            end
        end
    end

    function exports.end_reconcile()
        local pending = saved;
        saved = {};
        local changed = 0;

        for ent, keys in pairs(pending) do
            if not ent._removed then
                local networked = ent._networked;

                for key, old in pairs(keys) do
                    if rawequal(old, SAVED_NIL) then
                        old = nil;
                    end

                    local value = networked[key];

                    if not same(old, value) then
                        changed = changed + 1;
                        notify_changed(ent, key, old, value);
                    end
                end
            end
        end

        return changed;
    end

    function exports.owner_changed(raw, owner_raw)
        local ent = wrap(raw);

        if ent ~= nil then
            attach_owned(ent, owner_raw);
        end
    end

    function exports.set_local(raw)
        local_raw = raw;
    end

    return exports;
end
