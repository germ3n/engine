--[=[document
kind = "library",
name = "sound",
realm = "shared",
summary = "Plays sound definitions and files. The server replicates them. The client mixes them. Pitch 100 is normal speed.",
]=]
sound = {};

local function range(value, fallback)
    if type(value) == "table" then
        return value[1] or fallback, value[2] or value[1] or fallback;
    end

    if type(value) == "number" then
        return value, value;
    end

    return fallback, fallback;
end

local function waves(value)
    if type(value) == "string" then
        return { value };
    end

    if type(value) == "table" then
        return value;
    end

    return {};
end

--[=[document
parent = "sound",
name = "add",
realm = "shared",
summary = "Registers a sound definition. A name can pick a random wave, with volume and pitch ranges, a channel, a level, and a bus.",
params = {
    def = { ty = "table", desc = "Fields: name, sound or sounds, channel, level, volume, pitch, bus, loop, stream. channel is auto, weapon, voice, item, body, stream, or static. bus is sfx, music, ui, or voice. volume and pitch may be a number or {min, max}." },
},
returns = { ty = "nil", desc = "" },
example = "sound.add({\n    name = \"npc.mannequin.wave\",\n    channel = \"body\",\n    sound = \"sound/mannequin/wave.wav\",\n})",
see_also = "sound.play, Entity:emit_sound",
]=]
function sound.add(def)
    if type(def) ~= "table" or type(def.name) ~= "string" then
        return;
    end

    local vol_min, vol_max = range(def.volume, 1);
    local pitch_min, pitch_max = range(def.pitch, 100);
    sound._add(
        def.name,
        def.channel or "auto",
        def.level or 75,
        vol_min,
        vol_max,
        pitch_min,
        pitch_max,
        waves(def.sound or def.sounds),
        def.bus or "sfx",
        def.loop == true,
        def.stream == true
    );
end

--[=[document
parent = "sound",
name = "play",
realm = "shared",
summary = "Plays a definition or a file path. Without a position the sound is 2D on the interface bus. On the server this is sent to clients.",
params = {
    name = { ty = "string", desc = "Definition name from sound.add, or a file path such as sound/mannequin/wave.wav." },
    pos = { ty = "Vector3", desc = "World position. Omit it for a 2D interface sound.", optional = true },
    volume = { ty = "number", desc = "Loudness from 0 to 1. Omit it to use the definition.", optional = true },
    pitch = { ty = "number", desc = "Playback pitch. 100 is normal. Omit it to use the definition.", optional = true },
},
returns = { ty = "nil", desc = "" },
example = "sound.play(\"npc.mannequin.wave\", Vector3(0, 0, 0), 1, 100)",
see_also = "sound.add, Entity:emit_sound",
]=]
function sound.play(name, pos, volume, pitch)
    if pos == nil then
        sound._play(name, false, 0, 0, 0, volume or -1, pitch or -1, 0, "");

        return;
    end

    sound._play(name, true, pos.x, pos.y, pos.z, volume or -1, pitch or -1, 0, "");
end

function sound._emit(name, x, y, z, volume, pitch, handle, channel)
    sound._play(name, true, x, y, z, volume or -1, pitch or -1, handle or 0, channel or "");
end

function sound._stop(handle, name)
    sound._halt(handle or 0, name);
end

--[=[document
parent = "sound",
name = "scape_add",
realm = "shared",
summary = "Registers a soundscape. Its sounds are looping beds on the music bus, and its room picks the global reverb.",
params = {
    def = { ty = "table", desc = "Fields: name, room, sounds. room is none, room, hall, or underwater. sounds is a list of definition names." },
},
returns = { ty = "nil", desc = "" },
example = "sound.scape_add({ name = \"hall\", room = \"hall\", sounds = { \"ambient.hall\" } })",
see_also = "sound.scape_box, sound.set_room",
]=]
function sound.scape_add(def)
    if type(def) ~= "table" or type(def.name) ~= "string" then
        return;
    end

    sound._scape_add(def.name, def.room or "none", def.sounds or {});
end

--[=[document
parent = "sound",
name = "scape_box",
realm = "shared",
summary = "Places a soundscape in an axis-aligned box. The listener uses the first box that contains it.",
params = {
    name = { ty = "string", desc = "Name passed to sound.scape_add." },
    min = { ty = "Vector3", desc = "Minimum corner." },
    max = { ty = "Vector3", desc = "Maximum corner." },
},
returns = { ty = "nil", desc = "" },
example = "sound.scape_box(\"hall\", Vector3(-8, -8, 0), Vector3(8, 8, 4))",
see_also = "sound.scape_add",
]=]
function sound.scape_box(name, min, max)
    sound._scape_box(name, min.x, min.y, min.z, max.x, max.y, max.z);
end

--[=[document
parent = "sound",
name = "set_room",
realm = "shared",
summary = "Sets the global reverb. A soundscape box replaces it when the listener enters that box.",
params = {
    name = { ty = "string", desc = "none, room, hall, or underwater." },
},
returns = { ty = "nil", desc = "" },
example = "sound.set_room(\"underwater\")",
see_also = "sound.scape_add",
]=]
function sound.set_room(name)
    sound._set_room(name or "none");
end
