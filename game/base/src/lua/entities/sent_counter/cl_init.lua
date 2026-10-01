function ENT:on_spawn()
    self:log("spawned, count=" .. self:get_count());
end

function ENT:on_networked_changed(key, old, value)
    self:log("received " .. key .. " " .. tostring(old) .. " -> " .. tostring(value));
end
