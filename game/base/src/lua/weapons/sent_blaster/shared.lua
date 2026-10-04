local band = bit.band;
local IN_ATTACK = 1;
local IN_RELOAD = 128;
local IN_SPRINT = 8;
local IN_DUCK = 32;

WEAPON.base = "base_weapon";
WEAPON.print_name = "Blaster";
WEAPON.clip_size = 10;
WEAPON.fire_ticks = 12;
WEAPON.reload_ticks = 90;
WEAPON.model = "models/uzi.glb";

function WEAPON:log(message)
    local realm = SERVER and "SERVER" or "CLIENT";
    --print("[" .. realm .. "] " .. self:get_class() .. " #" .. self:index() .. ": " .. message);
end

function WEAPON:on_spawn()
    local owner = self:get_owner();
    local owner_name = owner and ("#" .. owner:index()) or "none";
    self:log("spawned, owner=" .. owner_name .. " ammo=" .. self:get_networked("ammo", 0));
    self:set_model(self.model);
    self:set_sequence("wpn_val_draw");
    self._pose = "draw";
end

function WEAPON:follow_owner(cmd)
    local owner = self:get_owner();

    if owner == nil or not owner:is_valid() then
        return;
    end

    local pos = owner:get_pos();
    local offset = owner:get_view_offset();

    if cmd ~= nil and band(cmd.buttons, IN_DUCK) ~= 0 then
        offset = owner:get_view_offset_ducked();
    end

    self:set_pos(Vector3(pos.x + offset.x, pos.y + offset.y, pos.z + offset.z));
    self:set_angles(owner:get_angles());
end

function WEAPON:finish_pose()
    print(SERVER and "SV" or "CL", engine.tick_count, self._pose, self:get_cycle(), engine.first_time_predicted);
    local pose = self._pose;

    if pose == nil or pose == "idle" or pose == "walk" or pose == "sprint" then
        return false;
    end

    if (self:get_cycle() or 0) < 0.99 then
        return true;
    end
    if engine.first_time_predicted then
        self:set_sequence("wpn_val_idle");
    end
    self._pose = "idle";

    return false;
end

function WEAPON:update_locomotion(cmd)
    local owner = self:get_owner();

    if owner == nil or not owner:is_valid() then
        return;
    end

    local velocity = owner:get_velocity();
    local speed2 = velocity.x * velocity.x + velocity.y * velocity.y;
    local sprinting = band(cmd.buttons, IN_SPRINT) ~= 0;
    local next_pose = "idle";
    local next_sequence = "wpn_val_idle";

    if speed2 > 8 then
        if sprinting then
            next_pose = "sprint";
            next_sequence = "wpn_val_sprint";
        else
            next_pose = "walk";
            next_sequence = "wpn_val_walk";
        end
    end

    if self._pose ~= next_pose then
        if engine.first_time_predicted then
            self:set_sequence(next_sequence);
        end
        self._pose = next_pose;
    end
end

function WEAPON:play_reload(ammo)
    local sequence = "wpn_val_reload";

    if ammo <= 0 then
        sequence = "wpn_val_reload_full";
    end

    if engine.first_time_predicted then
        self:set_sequence(sequence);
    end
    self._pose = sequence;
    local duration = self:sequence_duration(sequence);

    if duration ~= nil and duration > 0 then
        return math.max(1, math.floor(duration / engine.tick_interval + 0.5));
    end

    return self.reload_ticks;
end

function WEAPON:predicted_think(cmd)
    local tick = cmd.tick;
    local ammo = self:get_networked("ammo", self.clip_size);
    local next_fire = self:get_networked("next_fire", 0);

    self:follow_owner(cmd);

    if self:finish_pose() then
        return;
    end

    if tick < next_fire then
        return;
    end

    if band(cmd.buttons, IN_RELOAD) ~= 0 and ammo < self.clip_size then
        local wait = self:play_reload(ammo);
        self:set_networked("ammo", self.clip_size, true);
        self:set_networked("next_fire", tick + wait, true);

        if engine.first_time_predicted then
            self:log("tick " .. tick .. " reload, ammo " .. ammo .. " -> " .. self.clip_size);
        end

        return;
    end

    if band(cmd.buttons, IN_ATTACK) ~= 0 and ammo > 0 then
        self:set_networked("ammo", ammo - 1, true);
        self:set_networked("next_fire", tick + self.fire_ticks, true);

        print(SERVER, CLIENT, engine.first_time_predicted, self:handle(), self:get_owner())
        if engine.first_time_predicted then
            self:play_gesture("wpn_val_shoot");
            self:log("tick " .. tick .. " fire, ammo " .. ammo .. " -> " .. (ammo - 1));
        end

        return;
    end

    self:update_locomotion(cmd);
end
