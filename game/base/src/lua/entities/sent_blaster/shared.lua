local band = bit.band;
local IN_ATTACK = 1;
local IN_RELOAD = 128;

ENT.base = "base_entity";
ENT.print_name = "Blaster";
ENT.clip_size = 10;
ENT.fire_ticks = 12;
ENT.reload_ticks = 90;

function ENT:log(message)
    local realm = SERVER and "SERVER" or "CLIENT";
    --print("[" .. realm .. "] " .. self:get_class() .. " #" .. self:index() .. ": " .. message);
end

function ENT:on_spawn()
    local owner = self:get_owner();
    local owner_name = owner and ("#" .. owner:index()) or "none";
    self:log("spawned, owner=" .. owner_name .. " ammo=" .. self:get_networked("ammo", 0));
end

function ENT:predicted_think(cmd)
    local tick = cmd.tick;
    local ammo = self:get_networked("ammo", self.clip_size);
    local next_fire = self:get_networked("next_fire", 0);

    if tick < next_fire then
        return;
    end

    if band(cmd.buttons, IN_RELOAD) ~= 0 and ammo < self.clip_size then
        self:set_networked("ammo", self.clip_size, true);
        self:set_networked("next_fire", tick + self.reload_ticks, true);

        if engine.first_time_predicted then
            self:log("tick " .. tick .. " reload, ammo " .. ammo .. " -> " .. self.clip_size);
        end

        return;
    end

    if band(cmd.buttons, IN_ATTACK) ~= 0 and ammo > 0 then
        self:set_networked("ammo", ammo - 1, true);
        self:set_networked("next_fire", tick + self.fire_ticks, true);

        if engine.first_time_predicted then
            self:log("tick " .. tick .. " fire, ammo " .. ammo .. " -> " .. (ammo - 1));
        end
    end
end
