function ENT:initialize()
    self:set_networked("count", 0);
end

function ENT:on_spawn()
    self:log("spawned, count=" .. self:get_count());
    self:set_next_think(engine.curtime + self.think_rate);
end

function ENT:think()
    local old = self:get_count();
    local value = old + self.step;
    self:set_networked("count", value);
    self:log("set count " .. old .. " -> " .. value);
    self:set_next_think(engine.curtime + self.think_rate);
end
