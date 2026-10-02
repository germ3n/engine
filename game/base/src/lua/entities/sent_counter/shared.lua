ENT.base = "base_entity";
ENT.print_name = "Counter";
ENT.think_rate = 1;
ENT.step = 1;

function ENT:get_count()
    return self:get_networked("count", 0);
end

function ENT:log(message)
    local realm = SERVER and "SERVER" or "CLIENT";
    print("[" .. realm .. "] " .. self:get_class() .. " #" .. self:index() .. ": " .. message);
end
