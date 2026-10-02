local band = bit.band;
local IN_ATTACK = 1;
local IN_RELOAD = 128;

--[[hook.add("PlayerSpawned", "anim_rig", function(handle)
    local ply = ents.get_by_index(handle:index());

    if ply == nil then
        return;
    end

    ply:set_model("models/test.mdl", "models/test.anm");
    ply:set_sequence("idle");

    function ply:predicted_think(cmd)
        if band(cmd.buttons, IN_RELOAD) ~= 0 then
            self:set_sequence("lunge");

            return;
        end

        if band(cmd.buttons, IN_ATTACK) ~= 0 then
            self:play_gesture("wave");
        end

        local velocity = self:get_velocity();
        local speed = velocity.x * velocity.x + velocity.y * velocity.y;

        if speed > 0.05 then
            self:set_sequence("walk");
        else
            self:set_sequence("idle");
        end
    end

    function ply:on_anim_event(name)
        print("[anim] " .. tostring(name));
    end
end);]]