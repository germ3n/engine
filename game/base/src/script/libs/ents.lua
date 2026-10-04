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

--[=[document
kind = "library",
name = "ents",
realm = "shared",
summary = "Creates, finds, and removes entities.",
]=]
--[=[document
kind = "class",
name = "Entity",
realm = "shared",
summary = "A scripted or native entity. Call methods with a colon. Hooks are functions on the entity table.",
]=]
--[=[document
kind = "class",
name = "Player",
realm = "shared",
summary = "Native player entity. Shares Entity methods and adds hull and view-offset get/set.",
]=]
--[=[document
parent = "Entity",
name = "initialize",
kind = "hook",
realm = "shared",
summary = "Called after the entity is created. On the server this is ents.create. On the client this is the networked spawn.",
returns = { ty = "nil", desc = "" },
example = "function ENT:initialize()\nend",
]=]
--[=[document
parent = "Entity",
name = "on_spawn",
kind = "hook",
realm = "shared",
summary = "Called when the entity becomes spawned, after Entity:spawn on the server and after the client receives the spawn.",
returns = { ty = "nil", desc = "" },
see_also = "Entity:spawn",
]=]
--[=[document
parent = "Entity",
name = "think",
kind = "hook",
realm = "shared",
summary = "Called while the entity is spawned and engine.curtime has reached the time passed to set_next_think.",
returns = { ty = "nil", desc = "" },
see_also = "Entity:set_next_think",
]=]
--[=[document
parent = "Entity",
name = "predicted_think",
kind = "hook",
realm = "shared",
summary = "Called on the server and during client prediction with the command being simulated.",
params = {
    cmd = { ty = "table", desc = "Fields: tick, buttons, wish (Vector3), view (Angle3)." },
},
returns = { ty = "nil", desc = "" },
see_also = "engine.first_time_predicted",
]=]
--[=[document
parent = "Entity",
name = "on_remove",
kind = "hook",
realm = "shared",
summary = "Called once when the entity is removed.",
returns = { ty = "nil", desc = "" },
see_also = "Entity:remove",
]=]
--[=[document
parent = "Entity",
name = "on_networked_changed",
kind = "hook",
realm = "client",
summary = "Called when a replicated networked value changes, including after prediction reconcile.",
params = {
    key = { ty = "string", desc = "Value name." },
    old = { ty = "any", desc = "Previous value, or nil." },
    value = { ty = "any", desc = "New value." },
},
returns = { ty = "nil", desc = "" },
see_also = "Entity:set_networked, Entity:get_networked",
]=]
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
    local native_enable_physics = native.enable_physics;
    local native_set_mass = native.set_mass;
    local native_apply_impulse = native.apply_impulse;
    local native_set_owner = native.set_owner;
    local native_get_owner = native.get_owner;
    local native_set_model = native.set_model;
    local native_set_sequence = native.set_sequence;
    local native_set_sequence_id = native.set_sequence_id;
    local native_play_gesture = native.play_gesture;
    local native_stop_gesture = native.stop_gesture;
    local native_lookup_sequence = native.lookup_sequence;
    local native_get_sequence = native.get_sequence;
    local native_get_sequence_name = native.get_sequence_name;
    local native_sequence_count = native.sequence_count;
    local native_sequence_duration = native.sequence_duration;
    local native_sequence_loops = native.sequence_loops;
    local native_get_playback_rate = native.get_playback_rate;
    local native_get_cycle = native.get_cycle;
    local native_set_cycle = native.set_cycle;
    local native_reset_sequence = native.reset_sequence;
    local native_get_model = native.get_model;
    local native_get_clips = native.get_clips;
    local native_lookup_bone = native.lookup_bone;
    local native_get_bone_count = native.get_bone_count;
    local native_get_bone_name = native.get_bone_name;
    local native_get_bone_parent = native.get_bone_parent;
    local native_get_bone_position = native.get_bone_position;
    local native_get_bone_angles = native.get_bone_angles;
    local native_manipulate_bone_position = native.manipulate_bone_position;
    local native_manipulate_bone_angles = native.manipulate_bone_angles;
    local native_clear_bone_manipulations = native.clear_bone_manipulations;
    local native_get_hull = native.get_hull;
    local native_set_hull = native.set_hull;
    local native_get_hull_duck = native.get_hull_duck;
    local native_set_hull_duck = native.set_hull_duck;
    local native_get_view_offset = native.get_view_offset;
    local native_set_view_offset = native.set_view_offset;
    local native_get_view_offset_ducked = native.get_view_offset_ducked;
    local native_set_view_offset_ducked = native.set_view_offset_ducked;
    local attach_owned;

    local function report(ent, name, err)
        print("[ents] " .. tostring(ent._class) .. ":" .. name .. " error: " .. tostring(err));
    end

    local function is_class_table(value)
        if type(value) ~= "table" or type(value.class_name) ~= "string" then
            return false;
        end

        return scripted_ents.get(value.class_name) == value;
    end

    local function copy_value(value, seen)
        if type(value) ~= "table" or is_class_table(value) then
            return value;
        end

        local existing = seen[value];

        if existing ~= nil then
            return existing;
        end

        local out = {};
        seen[value] = out;

        for key, item in next, value do
            out[copy_value(key, seen)] = copy_value(item, seen);
        end

        local meta = getmetatable(value);

        if meta ~= nil then
            setmetatable(out, meta);
        end

        return out;
    end

    local function call_hook(ent, name)
        local callback = ent[name];

        if callback == nil then
            return;
        end

        local ok, err = pcall(callback, ent);

        if not ok then
            report(ent, name, err);
        end
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
            _interp_idx = 0,
            _spawned = spawned,
            _removed = false,
            _owner = 0,
        }, mt);

        if mt ~= native_meta then
            local seen = {};

            for key, value in pairs(mt) do
                if key ~= "__index" and type(value) ~= "function" then
                    ent[key] = copy_value(value, seen);
                end
            end
        end

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

    local interp_list = {};
    local interp_count = 0;

    local function remove_interp(ent)
        local idx = ent._interp_idx;

        if idx == nil or idx == 0 then
            return;
        end

        ent._interp_idx = 0;
        local moved = interp_list[interp_count];
        interp_list[interp_count] = nil;
        interp_count = interp_count - 1;

        if idx <= interp_count then
            interp_list[idx] = moved;
            moved._interp_idx = idx;
        end
    end

    local function add_interp(ent)
        if ent._interp_idx ~= nil and ent._interp_idx ~= 0 then
            return;
        end

        interp_count = interp_count + 1;
        interp_list[interp_count] = ent;
        ent._interp_idx = interp_count;
    end

    local function unlink(ent)
        ent._removed = true;
        remove_think(ent);
        remove_interp(ent);
        detach_owned(ent);
        saved[ent] = nil;

        if storage[ent._index] == ent then
            storage[ent._index] = nil;
        end
    end

    local function start(ent)
        if type(ent.think) == "function" then
            add_think(ent);
        end

        call_hook(ent, "on_spawn");
    end

    local bytes_t = ffi.typeof("uint8_t[?]");
    local u8_box = ffi.new("uint8_t[1]");
    local u16_box = ffi.new("uint16_t[1]");
    local u32_box = ffi.new("uint32_t[1]");
    local i32_box = ffi.new("int32_t[1]");
    local f32_box = ffi.new("float[1]");
    local f64_box = ffi.new("double[1]");
    local cap = 256;
    local raw = bytes_t(cap);
    local len = 0;

    local function reserve(n)
        local need = len + n;

        if need <= cap then
            return;
        end

        local grown = cap * 2;

        while need > grown do
            grown = grown * 2;
        end

        local next_raw = bytes_t(grown);
        ffi.copy(next_raw, raw, len);
        raw = next_raw;
        cap = grown;
    end

    local function write_pod(box, n)
        reserve(n);
        ffi.copy(raw + len, box, n);
        len = len + n;
    end

    local function write_u8(v)
        u8_box[0] = v;
        write_pod(u8_box, 1);
    end

    local function write_u16(v)
        u16_box[0] = v;
        write_pod(u16_box, 2);
    end

    local function write_u32(v)
        u32_box[0] = v;
        write_pod(u32_box, 4);
    end

    local function write_i32(v)
        i32_box[0] = v;
        write_pod(i32_box, 4);
    end

    local function write_f32(v)
        f32_box[0] = v;
        write_pod(f32_box, 4);
    end

    local function write_f64(v)
        f64_box[0] = v;
        write_pod(f64_box, 8);
    end

    local function patch_u16(at, v)
        u16_box[0] = v;
        ffi.copy(raw + at, u16_box, 2);
    end

    local function write_str(s)
        local n = #s;

        if n > 65535 then
            error("netvar string is too long", 2);
        end

        write_u16(n);

        if n == 0 then
            return;
        end

        reserve(n);
        ffi.copy(raw + len, s, n);
        len = len + n;
    end

    local function write_value(value)
        local kind = type(value);

        if kind == "nil" then
            write_u8(TAG_NIL);
        elseif kind == "boolean" then
            write_u8(TAG_BOOL);

            if value then
                write_u8(1);
            else
                write_u8(0);
            end
        elseif kind == "number" then
            if value == floor(value) and value >= INT_MIN and value <= INT_MAX then
                write_u8(TAG_INT);
                write_i32(value);
            else
                write_u8(TAG_FLOAT);
                write_f64(value);
            end
        elseif kind == "string" then
            write_u8(TAG_STRING);
            write_str(value);
        elseif kind == "table" then
            write_u8(TAG_ENTITY);
            write_u32(value._handle or 0);
        elseif ffi.istype(vector_type, value) then
            write_u8(TAG_VECTOR3);
            write_f64(value.x);
            write_f64(value.y);
            write_f64(value.z);
        else
            write_u8(TAG_ANGLE3);
            write_f32(value.p);
            write_f32(value.y);
            write_f32(value.r);
        end
    end

    local function write_var(key, value)
        write_str(key);
        write_value(value);
    end

    local function write_pairs(handle, map, values)
        write_u32(handle);
        local mark = len;
        write_u16(0);
        local count = 0;

        if values == nil then
            for key, value in pairs(map) do
                write_var(key, value);
                count = count + 1;
            end
        else
            for key in pairs(map) do
                write_var(key, values[key]);
                count = count + 1;
            end
        end

        if count > 65535 then
            error("too many netvars", 2);
        end

        patch_u16(mark, count);
    end

    local function begin_blob()
        len = 0;
    end

    local function take_blob()
        if len == 0 then
            return nil;
        end

        local out = ffi.string(raw, len);
        len = 0;

        return out;
    end

    local function need(at, size, n)
        if at + n > size then
            error("truncated netvar blob", 2);
        end
    end

    local function read_u8(data, at, size)
        need(at, size, 1);

        return tonumber(data[at]), at + 1;
    end

    local function read_u16(data, at, size)
        need(at, size, 2);

        return tonumber(data[at]) + tonumber(data[at + 1]) * 256, at + 2;
    end

    local function read_u32(data, at, size)
        need(at, size, 4);

        return tonumber(data[at])
            + tonumber(data[at + 1]) * 256
            + tonumber(data[at + 2]) * 65536
            + tonumber(data[at + 3]) * 16777216, at + 4;
    end

    local function read_i32(data, at, size)
        local v;
        v, at = read_u32(data, at, size);

        if v >= 2147483648 then
            v = v - 4294967296;
        end

        return v, at;
    end

    local function read_f32(data, at, size)
        need(at, size, 4);
        ffi.copy(f32_box, data + at, 4);

        return tonumber(f32_box[0]), at + 4;
    end

    local function read_f64(data, at, size)
        need(at, size, 8);
        ffi.copy(f64_box, data + at, 8);

        return tonumber(f64_box[0]), at + 8;
    end

    local function read_str(data, at, size)
        local n;
        n, at = read_u16(data, at, size);
        need(at, size, n);

        if n == 0 then
            return "", at;
        end

        return ffi.string(data + at, n), at + n;
    end

    local function open_blob(blob)
        return ffi.cast("const uint8_t*", blob), #blob;
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

    local function skip_value(data, at, size, tag)
        if tag == TAG_NIL then
            return at;
        elseif tag == TAG_BOOL then
            need(at, size, 1);

            return at + 1;
        elseif tag == TAG_INT or tag == TAG_ENTITY then
            need(at, size, 4);

            return at + 4;
        elseif tag == TAG_FLOAT then
            need(at, size, 8);

            return at + 8;
        elseif tag == TAG_STRING then
            local n;
            n, at = read_u16(data, at, size);
            need(at, size, n);

            return at + n;
        elseif tag == TAG_VECTOR3 then
            need(at, size, 24);

            return at + 24;
        elseif tag == TAG_ANGLE3 then
            need(at, size, 12);

            return at + 12;
        end

        error("bad netvar tag", 2);
    end

    local function read_value(data, at, size, tag)
        if tag == TAG_NIL then
            return nil, at;
        elseif tag == TAG_BOOL then
            local v;
            v, at = read_u8(data, at, size);

            return v ~= 0, at;
        elseif tag == TAG_INT then
            local v;
            v, at = read_i32(data, at, size);

            return v, at;
        elseif tag == TAG_FLOAT then
            local v;
            v, at = read_f64(data, at, size);

            return v, at;
        elseif tag == TAG_STRING then
            return read_str(data, at, size);
        elseif tag == TAG_VECTOR3 then
            local x, y, z;
            x, at = read_f64(data, at, size);
            y, at = read_f64(data, at, size);
            z, at = read_f64(data, at, size);

            return vector_type(x, y, z), at;
        elseif tag == TAG_ANGLE3 then
            local p, yaw, roll;
            p, at = read_f32(data, at, size);
            yaw, at = read_f32(data, at, size);
            roll, at = read_f32(data, at, size);

            return angle_type(p, yaw, roll), at;
        elseif tag == TAG_ENTITY then
            local handle;
            handle, at = read_u32(data, at, size);

            return wrap(handle), at;
        end

        error("bad netvar tag", 2);
    end

    local function skip_vars(data, at, size, count)
        for _ = 1, count do
            local tag;
            local ignored;
            ignored, at = read_str(data, at, size);
            tag, at = read_u8(data, at, size);
            at = skip_value(data, at, size, tag);
        end

        return at;
    end

    local function interp_enabled(ent, key)
        local keys = ent._interp;

        if type(keys) == "table" and keys[key] ~= nil then
            return keys[key] == true;
        end

        local marks = ent.interpolated;

        return type(marks) == "table" and not not marks[key];
    end

    local function can_lerp(value)
        local kind = type(value);

        if kind == "number" then
            return true;
        end

        if kind ~= "cdata" then
            return false;
        end

        return ffi.istype(vector_type, value) or ffi.istype(angle_type, value);
    end

    local function lerp_number(from, to, alpha)
        return from + (to - from) * alpha;
    end

    local function lerp_angle(from, to, alpha)
        local delta = (to - from) % 360;

        if delta > 180 then
            delta = delta - 360;
        end

        if delta < -180 then
            delta = delta + 360;
        end

        return from + delta * alpha;
    end

    local function lerp_value(from, to, alpha)
        if type(from) == "number" and type(to) == "number" then
            return lerp_number(from, to, alpha);
        end

        if ffi.istype(vector_type, from) and ffi.istype(vector_type, to) then
            return vector_type(
                lerp_number(from.x, to.x, alpha),
                lerp_number(from.y, to.y, alpha),
                lerp_number(from.z, to.z, alpha)
            );
        end

        if ffi.istype(angle_type, from) and ffi.istype(angle_type, to) then
            return angle_type(
                lerp_angle(from.p, to.p, alpha),
                lerp_angle(from.y, to.y, alpha),
                lerp_angle(from.r, to.r, alpha)
            );
        end

        return to;
    end

    local function sample_kind(value)
        if type(value) == "number" then
            return "n";
        end

        if type(value) == "cdata" and ffi.istype(vector_type, value) then
            return "v";
        end

        if type(value) == "cdata" and ffi.istype(angle_type, value) then
            return "a";
        end

        return "";
    end

    local function remember_sample(ent, key, time, value)
        local history = ent._samples;

        if history == nil then
            history = {};
            ent._samples = history;
        end

        local samples = history[key];

        if samples == nil then
            samples = {};
            history[key] = samples;
        end

        local last = samples[#samples];

        if last ~= nil and time <= last.time then
            time = last.time + 0.0001;
        end

        if last ~= nil and sample_kind(last.value) ~= sample_kind(value) then
            samples = { { time = time, value = value } };
            history[key] = samples;
        else
            samples[#samples + 1] = { time = time, value = value };

            while #samples > 32 do
                table.remove(samples, 1);
            end
        end

        add_interp(ent);

        return #samples;
    end

    local function blend_samples(samples, time)
        local count = #samples;

        if count == 0 then
            return nil;
        end

        local first = samples[1];

        if count == 1 or time <= first.time then
            return first.value;
        end

        local last = samples[count];

        if time >= last.time then
            return last.value;
        end

        local idx = 1;

        while idx + 1 < count and samples[idx + 1].time < time do
            idx = idx + 1;
        end

        local from = samples[idx];
        local to = samples[idx + 1];
        local span = to.time - from.time;
        local alpha = 1;

        if span > 1e-8 then
            alpha = (time - from.time) / span;

            if alpha < 0 then
                alpha = 0;
            end

            if alpha > 1 then
                alpha = 1;
            end
        end

        return lerp_value(from.value, to.value, alpha);
    end

    local function apply_vars(ent, data, at, size, count, notify, time)
        local networked = ent._networked;
        local predicted = nil;
        local skipped = 0;

        if local_raw ~= 0 and (ent._handle == local_raw or ent._owner == local_raw) then
            predicted = ent._predicted;
        end

        for _ = 1, count do
            local key, tag, value;
            key, at = read_str(data, at, size);
            tag, at = read_u8(data, at, size);

            if predicted == nil or not predicted[key] then
                value, at = read_value(data, at, size, tag);

                if time ~= nil and interp_enabled(ent, key) and can_lerp(value) then
                    local history = ent._samples;
                    local previous = networked[key];

                    if history ~= nil and history[key] ~= nil and #history[key] > 0 then
                        previous = history[key][#history[key]].value;
                    end

                    local stored = remember_sample(ent, key, time, value);

                    if stored == 1 then
                        networked[key] = value;
                    end

                    if notify and not same(previous, value) then
                        notify_changed(ent, key, previous, value);
                    end
                else
                    local old = networked[key];
                    networked[key] = value;

                    if notify and not same(old, value) then
                        notify_changed(ent, key, old, value);
                    end
                end
            else
                at = skip_value(data, at, size, tag);
                skipped = skipped + 1;
            end
        end

        return at, skipped;
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

    --[=[document
    parent = "Entity",
    name = "index",
    realm = "shared",
    summary = "Slot of this entity in the entity list.",
    returns = { ty = "number", desc = "Index used by ents.get_by_index." },
    ]=]
    function meta:index()
        return self._index;
    end

    --[=[document
    parent = "Entity",
    name = "handle",
    realm = "shared",
    summary = "Returns the entity handle userdata.",
    returns = { ty = "EntityHandle", desc = "Handle with index, generation, raw, and is_null." },
    ]=]
    function meta:handle()
        return native_handle(self._handle);
    end

    --[=[document
    parent = "Entity",
    name = "get_class",
    realm = "shared",
    summary = "Class name this entity was created as.",
    returns = { ty = "string", desc = "Registered class name." },
    ]=]
    function meta:get_class()
        return self._class;
    end

    --[=[document
    parent = "Entity",
    name = "is_valid",
    realm = "shared",
    summary = "False after the entity has been removed.",
    returns = { ty = "boolean", desc = "True while the entity is still in the list." },
    ]=]
    function meta:is_valid()
        return not self._removed;
    end

    --[=[document
    parent = "Entity",
    name = "spawn",
    realm = "shared",
    summary = "Marks the entity spawned and calls on_spawn. Does nothing if it is already spawned or removed.",
    returns = { ty = "nil", desc = "" },
    see_also = "Entity:on_spawn, ents.create",
    ]=]
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

    local function call_on_remove(ent)
        if ent._removing then
            return;
        end

        ent._removing = true;
        local callback = ent.on_remove;

        if callback ~= nil then
            local ok, err = pcall(callback, ent);

            if not ok then
                report(ent, "on_remove", err);
            end
        end
    end

    --[=[document
    parent = "Entity",
    name = "remove",
    realm = "shared",
    summary = "Calls on_remove and deletes the entity. A second call does nothing.",
    returns = { ty = "nil", desc = "" },
    see_also = "Entity:on_remove, ents.remove",
    ]=]
    function meta:remove()
        if self._removed then
            return;
        end

        call_on_remove(self);
        native_remove(self._handle);
        unlink(self);
    end

    --[=[document
    parent = "Entity",
    name = "get_pos",
    realm = "shared",
    summary = "World position.",
    returns = { ty = "Vector3", desc = "Current position." },
    see_also = "Entity:set_pos",
    ]=]
    function meta:get_pos()
        return vector_type(native_get_pos(self._handle));
    end

    --[=[document
    parent = "Entity",
    name = "set_pos",
    realm = "shared",
    summary = "Sets the world position.",
    params = {
        pos = { ty = "Vector3", desc = "New position." },
    },
    returns = { ty = "nil", desc = "" },
    example = "ent:set_pos(Vector3(0, 0, 64))",
    see_also = "Entity:get_pos",
    ]=]
    function meta:set_pos(pos)
        native_set_pos(self._handle, pos.x, pos.y, pos.z);
    end

    --[=[document
    parent = "Entity",
    name = "get_angles",
    realm = "shared",
    summary = "World angles.",
    returns = { ty = "Angle3", desc = "Pitch, yaw, and roll." },
    see_also = "Entity:set_angles",
    ]=]
    function meta:get_angles()
        return angle_type(native_get_angles(self._handle));
    end

    --[=[document
    parent = "Entity",
    name = "set_angles",
    realm = "shared",
    summary = "Sets the world angles.",
    params = {
        angles = { ty = "Angle3", desc = "New pitch, yaw, and roll." },
    },
    returns = { ty = "nil", desc = "" },
    see_also = "Entity:get_angles",
    ]=]
    function meta:set_angles(angles)
        native_set_angles(self._handle, angles.p, angles.y, angles.r);
    end

    --[=[document
    parent = "Entity",
    name = "get_velocity",
    realm = "shared",
    summary = "Velocity in units per second.",
    returns = { ty = "Vector3", desc = "Current velocity." },
    see_also = "Entity:set_velocity",
    ]=]
    function meta:get_velocity()
        return vector_type(native_get_velocity(self._handle));
    end

    --[=[document
    parent = "Entity",
    name = "set_velocity",
    realm = "shared",
    summary = "Sets the velocity.",
    params = {
        velocity = { ty = "Vector3", desc = "New velocity." },
    },
    returns = { ty = "nil", desc = "" },
    see_also = "Entity:get_velocity",
    ]=]
    function meta:set_velocity(velocity)
        native_set_velocity(self._handle, velocity.x, velocity.y, velocity.z);
    end

    --[=[document
    parent = "Entity",
    name = "enable_physics",
    realm = "shared",
    summary = "Gives the entity a dynamic box rigid body from its model bounds. Server only; returns false on the client or for players.",
    returns = { ty = "boolean", desc = "True when a dynamic body is attached." },
    see_also = "Entity:set_mass, Entity:apply_impulse",
    ]=]
    function meta:enable_physics()
        return native_enable_physics(self._handle);
    end

    --[=[document
    parent = "Entity",
    name = "set_mass",
    realm = "shared",
    summary = "Sets the mass of a dynamic physics body, in kilograms.",
    params = {
        mass = { ty = "number", desc = "Mass in kilograms. Must be positive." },
    },
    returns = { ty = "boolean", desc = "True when the body mass changed." },
    see_also = "Entity:enable_physics",
    ]=]
    function meta:set_mass(mass)
        return native_set_mass(self._handle, mass);
    end

    --[=[document
    parent = "Entity",
    name = "apply_impulse",
    realm = "shared",
    summary = "Applies an instant impulse to a dynamic physics body.",
    params = {
        impulse = { ty = "Vector3", desc = "Impulse in kilogram meters per second." },
    },
    returns = { ty = "boolean", desc = "True when the impulse was applied." },
    see_also = "Entity:enable_physics",
    ]=]
    function meta:apply_impulse(impulse)
        return native_apply_impulse(self._handle, impulse.x, impulse.y, impulse.z);
    end

    --[=[document
    parent = "Entity",
    name = "set_next_think",
    realm = "shared",
    summary = "Schedules the next think call.",
    params = {
        time = { ty = "number", desc = "engine.curtime at which think should run." },
    },
    returns = { ty = "nil", desc = "" },
    see_also = "Entity:think",
    ]=]
    function meta:set_next_think(time)
        self._next_think = time;
    end

    --[=[document
    parent = "Entity",
    name = "get_networked",
    realm = "shared",
    summary = "Reads a networked value stored on the entity.",
    params = {
        key = { ty = "string", desc = "Value name." },
        fallback = { ty = "any", desc = "Returned when the key has not been set.", optional = true },
    },
    returns = { ty = "any", desc = "The stored value, or fallback." },
    see_also = "Entity:set_networked",
    ]=]
    function meta:get_networked(key, fallback)
        local value = self._networked[key];

        if rawequal(value, nil) then
            return fallback;
        end

        return value;
    end

    --[=[document
    parent = "Entity",
    name = "set_owner",
    realm = "server",
    summary = "Sets the entity that owns this one. Owned entities are predicted with their owner.",
    params = {
        owner = { ty = "Entity", desc = "New owner, or nil to clear it.", optional = true },
    },
    returns = { ty = "nil", desc = "" },
    panics = "Errors on the client.",
    see_also = "Entity:get_owner",
    ]=]
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

    --[=[document
    parent = "Entity",
    name = "get_owner",
    realm = "shared",
    summary = "Entity that owns this one.",
    returns = { ty = "Entity", desc = "The owner, or nil." },
    see_also = "Entity:set_owner",
    ]=]
    function meta:get_owner()
        if self._owner == 0 then
            return nil;
        end

        return wrap(self._owner);
    end

    --[=[document
    parent = "Entity",
    name = "set_networked",
    realm = "server",
    summary = "Stores a value and, on the server, marks it for replication. A client call only updates the local copy.",
    params = {
        key = { ty = "string", desc = "Value name." },
        value = { ty = "any", desc = "nil, boolean, number, string, Entity, Vector3, or Angle3." },
        predicted = { ty = "boolean", desc = "When true, prediction keeps this key on the client.", optional = true },
    },
    returns = { ty = "nil", desc = "" },
    panics = "Errors if the key is not a string or the value type cannot be networked.",
    see_also = "Entity:get_networked, Entity:set_interpolated",
    ]=]
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

    --[=[document
    parent = "Entity",
    name = "set_interpolated",
    realm = "shared",
    summary = "Turns interpolation on or off for a networked key. Turning it off drops that key's sample history.",
    params = {
        key = { ty = "string", desc = "Networked value name." },
        enabled = { ty = "boolean", desc = "Defaults to true.", optional = true },
    },
    returns = { ty = "nil", desc = "" },
    panics = "Errors if the key is not a string.",
    see_also = "Entity:set_networked",
    ]=]
    function meta:set_interpolated(key, enabled)
        if type(key) ~= "string" then
            error("interpolated key must be a string", 2);
        end

        if enabled == nil then
            enabled = true;
        end

        local keys = self._interp;

        if keys == nil then
            keys = {};
            self._interp = keys;
        end

        keys[key] = enabled == true;

        if keys[key] then
            return;
        end

        local history = self._samples;

        if history ~= nil then
            history[key] = nil;
        end
    end

    --[=[document
    parent = "Entity",
    name = "set_model",
    realm = "shared",
    summary = "Sets the mesh and animation clip file. A glTF or GLB path supplies both when the clip path is omitted.",
    params = {
        mesh = { ty = "string", desc = "Model path, such as models/test.mdl or models/hero.glb." },
        clips = { ty = "string", desc = "Animation path, such as models/test.anm. For glTF or GLB, pass the same path or omit it.", optional = true },
    },
    returns = { ty = "nil", desc = "" },
    example = "ent:set_model(\"models/test.mdl\", \"models/test.anm\")",
    see_also = "Entity:set_sequence",
    ]=]
    function meta:set_model(mesh, clips)
        native_set_model(self._handle, mesh, clips or "");
    end

    --[=[document
    parent = "Entity",
    name = "set_sequence",
    realm = "shared",
    summary = "Plays a sequence from the entity's animation file.",
    params = {
        name = { ty = "string", desc = "Sequence name." },
        rate = { ty = "number", desc = "Playback rate. Defaults to 1.", optional = true },
    },
    returns = { ty = "nil", desc = "" },
    example = "ent:set_sequence(\"idle\")",
    see_also = "Entity:set_model, Entity:play_gesture",
    ]=]
    function meta:set_sequence(name, rate)
        native_set_sequence(self._handle, name, rate or 1);
    end

    --[=[document
    parent = "Entity",
    name = "set_sequence_id",
    realm = "shared",
    summary = "Plays a sequence by numeric id from the entity's animation file.",
    params = {
        id = { ty = "number", desc = "Sequence id from lookup_sequence." },
        rate = { ty = "number", desc = "Playback rate. Defaults to 1.", optional = true },
    },
    returns = { ty = "nil", desc = "" },
    see_also = "Entity:lookup_sequence, Entity:set_sequence",
    ]=]
    function meta:set_sequence_id(id, rate)
        native_set_sequence_id(self._handle, id, rate or 1);
    end

    --[=[document
    parent = "Entity",
    name = "lookup_sequence",
    realm = "shared",
    summary = "Returns the sequence id for a name, or nil.",
    params = {
        name = { ty = "string", desc = "Sequence name." },
    },
    returns = { ty = "number?", desc = "Sequence id." },
    see_also = "Entity:set_sequence_id, Entity:get_sequence_name",
    ]=]
    function meta:lookup_sequence(name)
        return native_lookup_sequence(self._handle, name);
    end

    --[=[document
    parent = "Entity",
    name = "get_sequence",
    realm = "shared",
    summary = "Returns the current sequence id, or nil.",
    returns = { ty = "number?", desc = "Current sequence id." },
    see_also = "Entity:get_sequence_name, Entity:set_sequence",
    ]=]
    function meta:get_sequence()
        return native_get_sequence(self._handle);
    end

    --[=[document
    parent = "Entity",
    name = "get_sequence_name",
    realm = "shared",
    summary = "Returns a sequence name. Omit the id to use the current sequence.",
    params = {
        id = { ty = "number", desc = "Sequence id.", optional = true },
    },
    returns = { ty = "string?", desc = "Sequence name." },
    see_also = "Entity:lookup_sequence, Entity:get_sequence",
    ]=]
    function meta:get_sequence_name(id)
        return native_get_sequence_name(self._handle, id);
    end

    --[=[document
    parent = "Entity",
    name = "sequence_count",
    realm = "shared",
    summary = "Returns how many sequences the animation file contains.",
    returns = { ty = "number", desc = "Sequence count." },
    ]=]
    function meta:sequence_count()
        return native_sequence_count(self._handle);
    end

    --[=[document
    parent = "Entity",
    name = "sequence_duration",
    realm = "shared",
    summary = "Returns sequence duration in seconds. Pass an id, a name, or omit for the current sequence.",
    params = {
        sequence = { ty = "number|string", desc = "Sequence id or name.", optional = true },
    },
    returns = { ty = "number?", desc = "Duration in seconds." },
    ]=]
    function meta:sequence_duration(sequence)
        return native_sequence_duration(self._handle, sequence);
    end

    --[=[document
    parent = "Entity",
    name = "sequence_loops",
    realm = "shared",
    summary = "Returns whether a sequence loops. Pass an id, a name, or omit for the current sequence.",
    params = {
        sequence = { ty = "number|string", desc = "Sequence id or name.", optional = true },
    },
    returns = { ty = "boolean?", desc = "True when the sequence loops." },
    ]=]
    function meta:sequence_loops(sequence)
        return native_sequence_loops(self._handle, sequence);
    end

    --[=[document
    parent = "Entity",
    name = "get_playback_rate",
    realm = "shared",
    summary = "Returns the current sequence playback rate.",
    returns = { ty = "number", desc = "Playback rate." },
    ]=]
    function meta:get_playback_rate()
        return native_get_playback_rate(self._handle);
    end

    --[=[document
    parent = "Entity",
    name = "get_cycle",
    realm = "shared",
    summary = "Returns the current sequence cycle from 0 to 1.",
    returns = { ty = "number", desc = "Cycle." },
    see_also = "Entity:set_cycle",
    ]=]
    function meta:get_cycle()
        return native_get_cycle(self._handle);
    end

    --[=[document
    parent = "Entity",
    name = "set_cycle",
    realm = "shared",
    summary = "Sets the current sequence cycle from 0 to 1.",
    params = {
        cycle = { ty = "number", desc = "Cycle from 0 to 1." },
    },
    returns = { ty = "nil", desc = "" },
    see_also = "Entity:get_cycle",
    ]=]
    function meta:set_cycle(cycle)
        native_set_cycle(self._handle, cycle);
    end

    --[=[document
    parent = "Entity",
    name = "reset_sequence",
    realm = "shared",
    summary = "Restarts the current sequence from the beginning.",
    returns = { ty = "nil", desc = "" },
    see_also = "Entity:set_sequence",
    ]=]
    function meta:reset_sequence()
        native_reset_sequence(self._handle);
    end

    --[=[document
    parent = "Entity",
    name = "get_model",
    realm = "shared",
    summary = "Returns the mesh path set by set_model.",
    returns = { ty = "string?", desc = "Mesh path." },
    see_also = "Entity:get_clips, Entity:set_model",
    ]=]
    function meta:get_model()
        return native_get_model(self._handle);
    end

    --[=[document
    parent = "Entity",
    name = "get_clips",
    realm = "shared",
    summary = "Returns the animation clip path set by set_model.",
    returns = { ty = "string?", desc = "Clip path." },
    see_also = "Entity:get_model, Entity:set_model",
    ]=]
    function meta:get_clips()
        return native_get_clips(self._handle);
    end

    --[=[document
    parent = "Entity",
    name = "lookup_bone",
    realm = "shared",
    summary = "Returns the bone index for a name, or nil.",
    params = {
        name = { ty = "string", desc = "Bone name." },
    },
    returns = { ty = "number?", desc = "Bone index." },
    ]=]
    function meta:lookup_bone(name)
        return native_lookup_bone(self._handle, name);
    end

    --[=[document
    parent = "Entity",
    name = "get_bone_count",
    realm = "shared",
    summary = "Returns how many bones the mesh contains.",
    returns = { ty = "number", desc = "Bone count." },
    ]=]
    function meta:get_bone_count()
        return native_get_bone_count(self._handle);
    end

    --[=[document
    parent = "Entity",
    name = "get_bone_name",
    realm = "shared",
    summary = "Returns the bone name for an index.",
    params = {
        bone = { ty = "number", desc = "Bone index." },
    },
    returns = { ty = "string?", desc = "Bone name." },
    ]=]
    function meta:get_bone_name(bone)
        return native_get_bone_name(self._handle, bone);
    end

    --[=[document
    parent = "Entity",
    name = "get_bone_parent",
    realm = "shared",
    summary = "Returns the parent bone index, or -1 for a root.",
    params = {
        bone = { ty = "number", desc = "Bone index." },
    },
    returns = { ty = "number?", desc = "Parent bone index." },
    ]=]
    function meta:get_bone_parent(bone)
        return native_get_bone_parent(self._handle, bone);
    end

    --[=[document
    parent = "Entity",
    name = "get_bone_position",
    realm = "shared",
    summary = "Returns the world position of a bone for the current pose.",
    params = {
        bone = { ty = "number", desc = "Bone index." },
    },
    returns = { ty = "Vector3?", desc = "World position." },
    see_also = "Entity:get_bone_angles",
    ]=]
    function meta:get_bone_position(bone)
        return native_get_bone_position(self._handle, bone);
    end

    --[=[document
    parent = "Entity",
    name = "get_bone_angles",
    realm = "shared",
    summary = "Returns the world angles of a bone for the current pose.",
    params = {
        bone = { ty = "number", desc = "Bone index." },
    },
    returns = { ty = "Angle3?", desc = "World angles." },
    see_also = "Entity:get_bone_position",
    ]=]
    function meta:get_bone_angles(bone)
        return native_get_bone_angles(self._handle, bone);
    end

    --[=[document
    parent = "Entity",
    name = "manipulate_bone_position",
    realm = "shared",
    summary = "Overrides a bone's local position for drawing.",
    params = {
        bone = { ty = "number", desc = "Bone index." },
        pos = { ty = "Vector3", desc = "Local position." },
    },
    returns = { ty = "nil", desc = "" },
    see_also = "Entity:manipulate_bone_angles, Entity:clear_bone_manipulations",
    ]=]
    function meta:manipulate_bone_position(bone, pos)
        native_manipulate_bone_position(self._handle, bone, pos.x, pos.y, pos.z);
    end

    --[=[document
    parent = "Entity",
    name = "manipulate_bone_angles",
    realm = "shared",
    summary = "Overrides a bone's local angles for drawing.",
    params = {
        bone = { ty = "number", desc = "Bone index." },
        angles = { ty = "Angle3", desc = "Local pitch, yaw, and roll." },
    },
    returns = { ty = "nil", desc = "" },
    see_also = "Entity:manipulate_bone_position, Entity:clear_bone_manipulations",
    ]=]
    function meta:manipulate_bone_angles(bone, angles)
        native_manipulate_bone_angles(self._handle, bone, angles.p, angles.y, angles.r);
    end

    --[=[document
    parent = "Entity",
    name = "clear_bone_manipulations",
    realm = "shared",
    summary = "Clears bone position and angle overrides.",
    returns = { ty = "nil", desc = "" },
    see_also = "Entity:manipulate_bone_position, Entity:manipulate_bone_angles",
    ]=]
    function meta:clear_bone_manipulations()
        native_clear_bone_manipulations(self._handle);
    end

    --[=[document
    parent = "Entity",
    name = "play_gesture",
    realm = "shared",
    summary = "Plays a gesture on top of the current sequence.",
    params = {
        name = { ty = "string", desc = "Gesture name." },
        rate = { ty = "number", desc = "Playback rate. Defaults to 1.", optional = true },
        weight = { ty = "number", desc = "Blend weight. Defaults to 1.", optional = true },
    },
    returns = { ty = "nil", desc = "" },
    see_also = "Entity:stop_gesture, Entity:set_sequence",
    ]=]
    function meta:play_gesture(name, rate, weight)
        native_play_gesture(self._handle, name, rate or 1, weight or 1);
    end

    --[=[document
    parent = "Entity",
    name = "stop_gesture",
    realm = "shared",
    summary = "Stops the gesture started by play_gesture.",
    returns = { ty = "nil", desc = "" },
    see_also = "Entity:play_gesture",
    ]=]
    function meta:stop_gesture()
        native_stop_gesture(self._handle);
    end

    --[=[document
    parent = "Player",
    name = "get_hull",
    realm = "shared",
    summary = "Standing collision mins and maxs.",
    returns = {
        { ty = "Vector3", desc = "Mins." },
        { ty = "Vector3", desc = "Maxs." },
    },
    example = "local mins, maxs = ply:get_hull()",
    see_also = "Player:set_hull, Player:get_hull_duck",
    ]=]
    function meta:get_hull()
        local min_x, min_y, min_z, max_x, max_y, max_z = native_get_hull(self._handle);

        return vector_type(min_x, min_y, min_z), vector_type(max_x, max_y, max_z);
    end

    --[=[document
    parent = "Player",
    name = "set_hull",
    realm = "shared",
    summary = "Sets standing collision mins and maxs.",
    params = {
        mins = { ty = "Vector3", desc = "Mins relative to origin." },
        maxs = { ty = "Vector3", desc = "Maxs relative to origin." },
    },
    returns = { ty = "nil", desc = "" },
    example = "ply:set_hull(Vector3(-0.28, -0.28, 0), Vector3(0.28, 0.28, 1.65))",
    see_also = "Player:get_hull, Player:set_hull_duck",
    ]=]
    function meta:set_hull(mins, maxs)
        native_set_hull(self._handle, mins.x, mins.y, mins.z, maxs.x, maxs.y, maxs.z);
    end

    --[=[document
    parent = "Player",
    name = "get_hull_duck",
    realm = "shared",
    summary = "Ducked collision mins and maxs.",
    returns = {
        { ty = "Vector3", desc = "Mins." },
        { ty = "Vector3", desc = "Maxs." },
    },
    example = "local mins, maxs = ply:get_hull_duck()",
    see_also = "Player:set_hull_duck, Player:get_hull",
    ]=]
    function meta:get_hull_duck()
        local min_x, min_y, min_z, max_x, max_y, max_z = native_get_hull_duck(self._handle);

        return vector_type(min_x, min_y, min_z), vector_type(max_x, max_y, max_z);
    end

    --[=[document
    parent = "Player",
    name = "set_hull_duck",
    realm = "shared",
    summary = "Sets ducked collision mins and maxs.",
    params = {
        mins = { ty = "Vector3", desc = "Mins relative to origin." },
        maxs = { ty = "Vector3", desc = "Maxs relative to origin." },
    },
    returns = { ty = "nil", desc = "" },
    example = "ply:set_hull_duck(Vector3(-0.28, -0.28, 0), Vector3(0.28, 0.28, 0.9))",
    see_also = "Player:get_hull_duck, Player:set_hull",
    ]=]
    function meta:set_hull_duck(mins, maxs)
        native_set_hull_duck(self._handle, mins.x, mins.y, mins.z, maxs.x, maxs.y, maxs.z);
    end

    --[=[document
    parent = "Player",
    name = "get_view_offset",
    realm = "shared",
    summary = "Eye offset from origin while standing.",
    returns = { ty = "Vector3", desc = "View offset." },
    example = "local offset = ply:get_view_offset()",
    see_also = "Player:set_view_offset, Player:get_view_offset_ducked",
    ]=]
    function meta:get_view_offset()
        return vector_type(native_get_view_offset(self._handle));
    end

    --[=[document
    parent = "Player",
    name = "set_view_offset",
    realm = "shared",
    summary = "Sets the standing eye offset from origin.",
    params = {
        offset = { ty = "Vector3", desc = "View offset." },
    },
    returns = { ty = "nil", desc = "" },
    example = "ply:set_view_offset(Vector3(0, 0, 1.45))",
    see_also = "Player:get_view_offset, Player:set_view_offset_ducked",
    ]=]
    function meta:set_view_offset(offset)
        native_set_view_offset(self._handle, offset.x, offset.y, offset.z);
    end

    --[=[document
    parent = "Player",
    name = "get_view_offset_ducked",
    realm = "shared",
    summary = "Eye offset from origin while ducked.",
    returns = { ty = "Vector3", desc = "View offset." },
    example = "local offset = ply:get_view_offset_ducked()",
    see_also = "Player:set_view_offset_ducked, Player:get_view_offset",
    ]=]
    function meta:get_view_offset_ducked()
        return vector_type(native_get_view_offset_ducked(self._handle));
    end

    --[=[document
    parent = "Player",
    name = "set_view_offset_ducked",
    realm = "shared",
    summary = "Sets the ducked eye offset from origin.",
    params = {
        offset = { ty = "Vector3", desc = "View offset." },
    },
    returns = { ty = "nil", desc = "" },
    example = "ply:set_view_offset_ducked(Vector3(0, 0, 0.78))",
    see_also = "Player:get_view_offset_ducked, Player:set_view_offset",
    ]=]
    function meta:set_view_offset_ducked(offset)
        native_set_view_offset_ducked(self._handle, offset.x, offset.y, offset.z);
    end

    --[=[document
    parent = "Entity",
    name = "emit_sound",
    realm = "shared",
    summary = "Plays a sound at this entity. On the server it is sent to clients. During prediction it plays on the first replay and the server echo is dropped. Pitch 100 is normal.",
    params = {
        name = { ty = "string", desc = "Definition name or file path." },
        volume = { ty = "number", desc = "Loudness from 0 to 1. Omit it to use the definition.", optional = true },
        pitch = { ty = "number", desc = "Playback pitch. 100 is normal.", optional = true },
        channel = { ty = "string", desc = "Replaces the definition channel. auto and static do not cut off the previous sound.", optional = true },
    },
    returns = { ty = "nil", desc = "" },
    example = "self:emit_sound(\"npc.mannequin.wave\")",
    see_also = "Entity:stop_sound, sound.play, sound.add",
    ]=]
    function meta:emit_sound(name, volume, pitch, channel)
        local pos = self:get_pos();
        sound._emit(name, pos.x, pos.y, pos.z, volume, pitch, self._handle, channel);
    end

    --[=[document
    parent = "Entity",
    name = "stop_sound",
    realm = "shared",
    summary = "Stops sounds started on this entity. Without a name, every sound on the entity stops.",
    params = {
        name = { ty = "string", desc = "Definition name or file path. Omit it to stop every sound on this entity.", optional = true },
    },
    returns = { ty = "nil", desc = "" },
    example = "self:stop_sound(\"npc.mannequin.wave\")",
    see_also = "Entity:emit_sound",
    ]=]
    function meta:stop_sound(name)
        sound._stop(self._handle, name);
    end

    --[=[document
    parent = "ents",
    name = "create",
    realm = "server",
    summary = "Creates a scripted entity and calls initialize.",
    params = {
        class = { ty = "string", desc = "Class passed to scripted_ents.register." },
    },
    returns = { ty = "Entity", desc = "The new entity, or nil if the list could not spawn it." },
    example = "local ent = ents.create(\"sent_blaster\")\nent:spawn()",
    panics = "Errors on the client, if the class is not registered, or if the class is a native class.",
    see_also = "Entity:spawn, Entity:initialize, scripted_ents.register",
    ]=]
    function ents.create(class)
        if CLIENT then
            error("create is server only", 2);
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
        call_hook(ent, "initialize");

        return ent;
    end

    --[=[document
    parent = "ents",
    name = "get_by_index",
    realm = "shared",
    summary = "Finds an entity by its list index.",
    params = {
        index = { ty = "number", desc = "Index from Entity:index." },
    },
    returns = { ty = "Entity", desc = "The entity, or nil." },
    see_also = "Entity:index",
    ]=]
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

    --[=[document
    parent = "ents",
    name = "get_all",
    realm = "shared",
    summary = "Every scripted entity currently in the list.",
    returns = { ty = "table", desc = "Array of Entity. Native entities without a scripted class are left out." },
    ]=]
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

            if ent ~= nil and scripted_ents.get_stored(ent._class) ~= nil then
                count = count + 1;
                out[count] = ent;
            end
        end

        all = out;
        all_revision = revision;

        return out;
    end

    --[=[document
    parent = "ents",
    name = "get",
    realm = "shared",
    summary = "Resolves an entity from a table, a numeric handle, or a value with a raw method.",
    params = {
        value = { ty = "any", desc = "Entity table, numeric handle, or userdata with raw()." },
    },
    returns = { ty = "Entity", desc = "The entity, or nil if it is missing or already removed." },
    ]=]
    function ents.get(value)
        if type(value) == "table" then
            if value._removed then
                return nil;
            end

            return value;
        end

        if type(value) == "number" then
            return wrap(value);
        end

        if value ~= nil and value.raw ~= nil then
            return wrap(value:raw());
        end

        return nil;
    end

    --[=[document
    parent = "ents",
    name = "remove",
    realm = "shared",
    summary = "Resolves an entity and removes it.",
    params = {
        value = { ty = "any", desc = "Same values ents.get accepts." },
    },
    returns = { ty = "nil", desc = "" },
    see_also = "ents.get, Entity:remove",
    ]=]
    function ents.remove(value)
        local ent = ents.get(value);

        if ent == nil then
            return;
        end

        ent:remove();
    end

    --[=[document
    parent = "ents",
    name = "get_by_class",
    realm = "shared",
    summary = "Scripted entities whose class name matches.",
    params = {
        class = { ty = "string", desc = "Class name." },
    },
    returns = { ty = "table", desc = "Array of Entity." },
    see_also = "ents.find_by_class, ents.get_all",
    ]=]
    function ents.get_by_class(class)
        local list = ents.get_all();
        local out = {};
        local count = 0;

        for idx = 1, #list do
            local ent = list[idx];

            if ent._class == class and scripted_ents.get_stored(ent._class) ~= nil then
                count = count + 1;
                out[count] = ent;
            end
        end

        return out;
    end

    --[=[document
    parent = "ents",
    name = "find_by_class",
    realm = "shared",
    summary = "Alias of ents.get_by_class.",
    params = {
        class = { ty = "string", desc = "Class name." },
    },
    returns = { ty = "table", desc = "Array of Entity." },
    see_also = "ents.get_by_class",
    ]=]
    function ents.find_by_class(class)
        return ents.get_by_class(class);
    end

    --[=[document
    parent = "ents",
    name = "get_count",
    realm = "shared",
    summary = "Number of entities in the list, including ones ents.get_all skips.",
    returns = { ty = "number", desc = "Entity count." },
    see_also = "ents.get_all",
    ]=]
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
                call_on_remove(ent);
                unlink(ent);
            end
        end
    end

    function exports.net_spawn(raw, blob, time)
        local class_hash = native_class_hash(raw);

        if class_hash == nil or scripted_ents._by_hash[class_hash] == nil then
            return false;
        end

        local ent = wrap(raw);
        ent._spawned = true;

        if type(blob) == "string" and #blob > 0 then
            local data, size = open_blob(blob);
            local count, at = read_u16(data, 0, size);
            apply_vars(ent, data, at, size, count, false, time);
        end

        call_hook(ent, "initialize");
        start(ent);

        return true;
    end

    function exports.collect_networked()
        if next(dirty) == nil then
            return nil;
        end

        begin_blob();

        for ent, keys in pairs(dirty) do
            write_pairs(ent._handle, keys, ent._networked);
        end

        for ent in pairs(dirty) do
            dirty[ent] = nil;
        end

        return take_blob();
    end

    function exports.networked_state(raw)
        begin_blob();

        if raw ~= nil then
            local ent = storage[raw % INDEX_SPAN];

            if ent == nil or ent._handle ~= raw then
                return nil;
            end

            if next(ent._networked) ~= nil then
                write_pairs(ent._handle, ent._networked);
            end
        else
            for _, ent in pairs(storage) do
                if ent._spawned and next(ent._networked) ~= nil then
                    write_pairs(ent._handle, ent._networked);
                end
            end
        end

        return take_blob();
    end

    function exports.apply_networked(blob, time)
        if type(blob) ~= "string" or #blob == 0 then
            return 0, 0;
        end

        local data, size = open_blob(blob);
        local at = 0;
        local skipped = 0;
        local missing = 0;

        while at < size do
            local raw, count;
            raw, at = read_u32(data, at, size);
            count, at = read_u16(data, at, size);
            local ent = wrap(raw);

            if ent ~= nil then
                local ent_skipped;
                at, ent_skipped = apply_vars(ent, data, at, size, count, true, time);
                skipped = skipped + ent_skipped;
            else
                at = skip_vars(data, at, size, count);
                missing = missing + 1;
            end
        end

        return skipped, missing;
    end

    function exports.present_interpolated(time)
        local idx = 1;

        while idx <= interp_count do
            local ent = interp_list[idx];

            if ent == nil or ent._removed then
                idx = idx + 1;
            else
                local history = ent._samples;

                if history ~= nil then
                    for key, samples in pairs(history) do
                        while #samples > 2 and samples[2].time < time do
                            table.remove(samples, 1);
                        end

                        local value = blend_samples(samples, time);

                        if not rawequal(value, nil) then
                            ent._networked[key] = value;
                        end
                    end
                end

                idx = idx + 1;
            end
        end
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
            return nil;
        end

        begin_blob();

        if ent._predicted ~= nil then
            write_pairs(ent._handle, ent._predicted, ent._networked);
        end

        local owned = ent._owned;

        if owned ~= nil then
            for idx = 1, #owned do
                local child = owned[idx];

                if child ~= nil and child._predicted ~= nil then
                    write_pairs(child._handle, child._predicted, child._networked);
                end
            end
        end

        return take_blob();
    end

    function exports.begin_reconcile(blob)
        saved = {};

        if type(blob) ~= "string" or #blob == 0 then
            return;
        end

        local data, size = open_blob(blob);
        local at = 0;

        while at < size do
            local raw, count;
            raw, at = read_u32(data, at, size);
            count, at = read_u16(data, at, size);
            local ent = wrap(raw);

            if ent == nil then
                at = skip_vars(data, at, size, count);
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
                    local key, tag, value;
                    key, at = read_str(data, at, size);
                    tag, at = read_u8(data, at, size);
                    value, at = read_value(data, at, size, tag);
                    predicted[key] = true;

                    if rawequal(keys[key], nil) then
                        local old = networked[key];

                        if rawequal(old, nil) then
                            old = SAVED_NIL;
                        end

                        keys[key] = old;
                    end

                    networked[key] = value;
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

    function exports.anim_event(raw, name)
        local ent = wrap(raw);

        if ent == nil or ent._removed then
            return;
        end

        local callback = ent.on_anim_event;

        if callback == nil then
            return;
        end

        local ok, err = pcall(callback, ent, name);

        if not ok then
            report(ent, "on_anim_event", err);
        end
    end

    return exports;
end
