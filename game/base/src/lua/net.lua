net = {};
net._storage = net._storage or {};
net._names = net._names or {};
local storage = net._storage;
local names = net._names;
            
function net.add_callback(umsg_name, callback)
    local hash = net.hash(umsg_name);
    storage[hash] = callback;
    names[hash] = umsg_name;
end
            
function net.call(hash, ...)
    if storage[hash] then
        storage[hash](...);
    end
end

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