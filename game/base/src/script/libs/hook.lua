--[=[document
kind = "library",
name = "hook",
realm = "shared",
summary = "Named callbacks. hook.call runs every callback for an event and returns the first non-nil result.",
]=]
--[=[document
parent = "hook",
name = "Initialize",
kind = "hook",
realm = "server",
summary = "Called once when the server loop starts.",
returns = { ty = "nil", desc = "" },
example = "hook.add(\"Initialize\", \"spawn\", function()\nend)",
]=]
--[=[document
parent = "hook",
name = "PlayerConnected",
kind = "hook",
realm = "client",
summary = "Called when a player joins.",
params = {
    handle = { ty = "EntityHandle", desc = "The player. ents.get accepts it." },
    name = { ty = "string", desc = "Player name." },
},
returns = { ty = "nil", desc = "" },
see_also = "hook.PlayerDisconnected, ents.get",
]=]
--[=[document
parent = "hook",
name = "PlayerDisconnected",
kind = "hook",
realm = "client",
summary = "Called when a player leaves.",
params = {
    handle = { ty = "EntityHandle", desc = "The player. ents.get accepts it." },
},
returns = { ty = "nil", desc = "" },
see_also = "hook.PlayerConnected",
]=]
--[=[document
parent = "hook",
name = "PlayerSpawned",
kind = "hook",
realm = "shared",
summary = "Called on the server when that player's spawn is sent, and on the client when the local player spawns.",
params = {
    handle = { ty = "EntityHandle", desc = "The player. ents.get accepts it." },
},
returns = { ty = "nil", desc = "" },
example = "hook.add(\"PlayerSpawned\", \"give\", function(handle)\nend)",
see_also = "ents.get",
]=]
--[=[document
parent = "hook",
name = "PlayerDamaged",
kind = "hook",
realm = "client",
summary = "Called when a player takes damage.",
params = {
    handle = { ty = "EntityHandle", desc = "The player who was hit." },
    attacker = { ty = "EntityHandle", desc = "The attacker." },
    inflictor = { ty = "EntityHandle", desc = "The entity that applied the damage." },
    damage = { ty = "number", desc = "Damage amount." },
    new_health = { ty = "number", desc = "Health after the hit." },
},
returns = { ty = "nil", desc = "" },
]=]
--[=[document
parent = "hook",
name = "PlayerDied",
kind = "hook",
realm = "client",
summary = "Called when a player dies.",
params = {
    handle = { ty = "EntityHandle", desc = "The player who died." },
    killer = { ty = "EntityHandle", desc = "The killer." },
    inflictor = { ty = "EntityHandle", desc = "The entity that applied the killing damage." },
},
returns = { ty = "nil", desc = "" },
]=]
--[=[document
parent = "hook",
name = "ModelChanged",
kind = "hook",
realm = "client",
summary = "Called when an entity's model changes.",
params = {
    handle = { ty = "EntityHandle", desc = "The entity." },
    model = { ty = "string", desc = "Model path." },
},
returns = { ty = "nil", desc = "" },
]=]
--[=[document
parent = "hook",
name = "TransformUpdated",
kind = "hook",
realm = "client",
summary = "Called when an entity's networked transform arrives.",
params = {
    handle = { ty = "EntityHandle", desc = "The entity." },
    position = { ty = "Vector3", desc = "World position, or nil when this update omitted it.", optional = true },
    angles = { ty = "Angle3", desc = "World angles, or nil when this update omitted them.", optional = true },
    velocity = { ty = "Vector3", desc = "World velocity, or nil when this update omitted it.", optional = true },
},
returns = { ty = "nil", desc = "" },
]=]
--[=[document
parent = "hook",
name = "ChatMessage",
kind = "hook",
realm = "client",
summary = "Called when a chat line arrives.",
params = {
    sender = { ty = "EntityHandle", desc = "The sender." },
    team_only = { ty = "boolean", desc = "True when the line is team chat." },
    text = { ty = "string", desc = "The message." },
},
returns = { ty = "nil", desc = "" },
]=]
--[=[document
parent = "hook",
name = "VoiceChunk",
kind = "hook",
realm = "client",
summary = "Called when a voice packet arrives.",
params = {
    sender = { ty = "EntityHandle", desc = "The speaker." },
    data = { ty = "table", desc = "1-indexed list of byte values." },
},
returns = { ty = "nil", desc = "" },
]=]
--[=[document
parent = "hook",
name = "MenuPaint",
kind = "hook",
realm = "client",
summary = "Called each frame before queued surface commands are drawn.",
returns = { ty = "nil", desc = "" },
example = "hook.add(\"MenuPaint\", \"hud\", function()\n    surface.draw_rect(8, 8, 32, 32, 255, 255, 255, 255)\nend)",
see_also = "surface.draw_rect",
]=]
--[=[document
parent = "hook",
name = "GuiMousePressed",
kind = "hook",
realm = "client",
summary = "Called when a mouse button goes down. Return true to keep the click in the UI. Otherwise the game may capture the cursor.",
params = {
    button = { ty = "number", desc = "1 is left, 2 is right, 3 is middle." },
    x = { ty = "number", desc = "Cursor x in pixels." },
    y = { ty = "number", desc = "Cursor y in pixels." },
},
returns = { ty = "boolean", desc = "True consumes the press. Nil lets the game handle it." },
see_also = "gui.hit, input.mouse_down",
]=]
--[=[document
parent = "hook",
name = "GuiKeyPressed",
kind = "hook",
realm = "client",
summary = "Called when a key goes down. Return true so the game does not also treat it as a bind.",
params = {
    name = { ty = "string", desc = "Key name, the same names as input.key_down." },
    repeated = { ty = "boolean", desc = "True when the key is repeating." },
},
returns = { ty = "boolean", desc = "True consumes the key. Nil lets the game handle it." },
see_also = "input.key_down, hook.GuiText",
]=]
--[=[document
parent = "hook",
name = "GuiText",
kind = "hook",
realm = "client",
summary = "Called when text is typed. Return a non-nil value to stop later callbacks. The characters are still stored for input.typed.",
params = {
    text = { ty = "string", desc = "The inserted characters." },
},
returns = { ty = "boolean", desc = "A non-nil return stops later callbacks." },
see_also = "input.typed, hook.GuiKeyPressed",
]=]
hook = {};
hook._storage = hook._storage or {};
local storage = hook._storage;

--[=[document
parent = "hook",
name = "add",
realm = "shared",
summary = "Registers a callback for an event. A later add with the same identifier replaces the previous callback.",
params = {
    event_id = { ty = "string", desc = "Event name." },
    identifier = { ty = "string", desc = "Name that identifies this callback." },
    callback = { ty = "function", desc = "Called by hook.call with the event arguments." },
},
returns = { ty = "nil", desc = "" },
example = "hook.add(\"PlayerSpawned\", \"anim_rig\", function(handle)\nend)",
see_also = "hook.call",
]=]
function hook.add(event_id, identifier, callback)
    if not storage[event_id] then
        storage[event_id] = {};
    end
    storage[event_id][identifier] = callback;
end

--[=[document
parent = "hook",
name = "call",
realm = "shared",
summary = "Runs every callback registered for an event. Stops at the first callback that returns non-nil.",
params = {
    event_id = { ty = "string", desc = "Event name." },
    args = { ty = "any", desc = "Arguments forwarded to each callback.", optional = true },
},
returns = { ty = "any", desc = "Up to six return values from the first callback that returned non-nil." },
example = "hook.call(\"PlayerSpawned\", handle)",
see_also = "hook.add",
]=]
function hook.call(event_id, ...)
    if storage[event_id] then
        for identifier, callback in pairs(storage[event_id]) do
            local a, b, c, d, e, f = callback(...);
            if a ~= nil then
                return a, b, c, d, e, f;
            end
        end
    end
end
