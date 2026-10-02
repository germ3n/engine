--[=[document
kind = "library",
name = "hook",
realm = "shared",
summary = "Named callbacks. hook.call runs every callback for an event and returns the first non-nil result.",
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
