function ENT:on_spawn()
    self:set_model("models/test.mdl", "models/test.anm");
    self:set_sequence("walk");
    self._wave_at = engine.curtime + 4;
    self:set_next_think(engine.curtime);
end

function ENT:think()
    local ply = self.follow;

    if ply ~= nil and ply:is_valid() then
        local look = ply:get_angles();
        local yaw = math.rad(look.y);
        local forward = Vector3(math.cos(yaw), math.sin(yaw), 0);
        local right = Vector3(math.sin(yaw), -math.cos(yaw), 0);
        local spacing = 1.6;
        local column = self.column or 0;
        local row = self.row or 0;
        self:set_pos(ply:get_pos() + forward * (3.5 + row * spacing) + right * ((column - 4.5) * spacing));
        self:set_angles(Angle3(0, look.y + 180, 0));
    end

    if engine.curtime >= (self._wave_at or 0) then
        self:play_gesture("wave");
        self:emit_sound("npc.mannequin.wave");
        self._wave_at = engine.curtime + 4;
    end

    self:set_next_think(engine.curtime);
end
