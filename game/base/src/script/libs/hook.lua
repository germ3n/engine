hook = {};
hook._storage = hook._storage or {};
local storage = hook._storage;
            
function hook.add(event_id, identifier, callback)
    if not storage[event_id] then
        storage[event_id] = {};
    end
    storage[event_id][identifier] = callback;
end
            
function hook.call(event_id, ...)
    if storage[event_id] then
        for identifier, callback in pairs(storage[event_id]) do
            callback(...);
        end
    end
end
