function ENT:on_spawn()
    self:set_model("models/test.mdl", "models/test.anm");
    self:set_sequence("walk");
    self._wave_at = engine.curtime + 4;
    self._next_path = 0;
    self._path = nil;
    self._path_idx = 1;
    self:set_next_think(engine.curtime);

    local answers = ai.ask("A player is aiming at you from 4 meters and has not fired.", {
        { "Should I move?", { "forward", "back", "strafe", "stay" } },
        { "Should I attack?", { "shoot", "melee", "no" } },
    })
    print("[ai] move " .. answers[1].choice .. " " .. answers[1].confidence)
    print("[ai] attack " .. answers[2].choice .. " " .. answers[2].confidence)
end

function ENT:think()
    local ply = self.follow;

    if ply ~= nil and ply:is_valid() and nav.ready() then
        local goal = ply:get_pos();
        local now = engine.curtime;
        local missing = self._path == nil or #self._path == 0;
        local moved = false;

        if not missing then
            local last = self._path[#self._path];
            local dx = goal.x - last.x;
            local dy = goal.y - last.y;
            local dz = goal.z - last.z;
            moved = dx * dx + dy * dy + dz * dz > 1;
        end

        if (missing or moved) and now >= (self._next_path or 0) then
            self._path = nav.path(self:get_pos(), goal);
            self._path_idx = 1;
            self._next_path = now + 0.5;

            if self._path == nil then
                nav.set_follow({});
            else
                nav.set_follow(self._path);
            end
        end

        self:step_path();
    end

    if engine.curtime >= (self._wave_at or 0) then
        self:play_gesture("wave");
        self:emit_sound("npc.mannequin.wave");
        self._wave_at = engine.curtime + 4;
    end

    self:set_next_think(engine.curtime);
end

function ENT:step_path()
    local path = self._path;

    if path == nil or #path == 0 then
        return;
    end

    local left = 8 * (engine.frametime or 0);
    local pos = self:get_pos();
    local moved_x = 0;
    local moved_y = 0;

    while left > 0 and self._path_idx <= #path do
        local point = path[self._path_idx];
        local dx = point.x - pos.x;
        local dy = point.y - pos.y;
        local dz = point.z - pos.z;
        local dist = math.sqrt(dx * dx + dy * dy + dz * dz);

        if dist <= 0.05 or dist <= left then
            moved_x = moved_x + dx;
            moved_y = moved_y + dy;
            pos = point;
            left = left - dist;
            self._path_idx = self._path_idx + 1;
        else
            local scale = left / dist;
            moved_x = moved_x + dx * scale;
            moved_y = moved_y + dy * scale;
            pos = Vector3(pos.x + dx * scale, pos.y + dy * scale, pos.z + dz * scale);
            left = 0;
        end
    end

    self:set_pos(pos);

    if moved_x * moved_x + moved_y * moved_y > 0.00000001 then
        self:set_angles(Angle3(0, math.deg(math.atan2(moved_y, moved_x)), 0));
    end
end
