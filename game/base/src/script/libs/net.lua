--[=[document
kind = "library",
name = "net",
realm = "shared",
summary = "User messages and name hashes. net.writer and net.send move bytes to the other side. net.hash is the id used for callbacks and entity class names.",
]=]
net = {};
net._storage = net._storage or {};
net._names = net._names or {};
local storage = net._storage;
local names = net._names;
            
--[=[document
parent = "net",
name = "add_callback",
realm = "shared",
summary = "Stores a callback under the hash of a usermessage name.",
params = {
    umsg_name = { ty = "string", desc = "Message name passed to net.send." },
    callback = { ty = "function", desc = "Called by net.call with the message arguments." },
},
returns = { ty = "nil", desc = "" },
example = "net.add_callback(\"hit\", function()\nend)",
see_also = "net.send, net.hash",
]=]
function net.add_callback(umsg_name, callback)
    local hash = net.hash(umsg_name);
    storage[hash] = callback;
    names[hash] = umsg_name;
end
            
--[=[document
parent = "net",
name = "call",
realm = "shared",
summary = "Runs the callback stored for a usermessage hash.",
params = {
    hash = { ty = "number", desc = "Hash from net.hash or the message id." },
    args = { ty = "any", desc = "Arguments forwarded to the callback.", optional = true },
},
returns = { ty = "nil", desc = "" },
see_also = "net.add_callback",
]=]
function net.call(hash, ...)
    if storage[hash] then
        storage[hash](...);
    end
end

--[=[document
parent = "net",
name = "hash_to_name",
realm = "shared",
summary = "Returns the usermessage name last registered for a hash.",
params = {
    hash = { ty = "number", desc = "Hash passed to net.add_callback." },
},
returns = { ty = "string", desc = "The name, or nil if the hash was never registered." },
see_also = "net.add_callback, net.hash",
]=]
function net.hash_to_name(hash)
    return names[hash];
end

local bit = require("bit")

local function mul32_fnv(a)
    local a_lo = a % 65536
    local a_hi = math.floor(a / 65536)
    
    local res = (a_lo * 403) + ((a_hi * 403 + a_lo * 256) % 65536) * 65536
    return res % 4294967296
end

--[=[document
parent = "net",
name = "hash",
realm = "shared",
summary = "FNV-1a hash of a string. Entity class names and usermessage names use this id.",
params = {
    str = { ty = "string", desc = "Text to hash." },
},
returns = { ty = "number", desc = "Unsigned 32-bit hash." },
example = "local id = net.hash(\"sent_blaster\")",
]=]
function net.hash(str)
    local hash = 2166136261
    
    for idx = 1, #str do
        hash = bit.bxor(hash, string.byte(str, idx))
        
        if hash < 0 then
            hash = hash + 4294967296
        end
        
        hash = mul32_fnv(hash)
    end
    
    return hash
end