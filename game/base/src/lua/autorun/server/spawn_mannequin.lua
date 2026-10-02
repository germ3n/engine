local spawned = false;
local count = 0;
local columns = 10;
local spacing = 1.6;

hook.add("PlayerSpawned", "spawn_mannequin", function(handle)
    if spawned then
        return;
    end

    local ply = ents.get_by_index(handle:index());

    if ply == nil then
        return;
    end

    spawned = true;
    local look = ply:get_angles();
    local idx = 0;

    while idx < count do
        local mannequin = ents.create("sent_mannequin");

        if mannequin == nil then
            print("[anim] failed to create sent_mannequin");

            return;
        end

        mannequin.follow = ply;
        mannequin.column = idx % columns;
        mannequin.row = math.floor(idx / columns);
        mannequin:set_pos(ply:get_pos());
        mannequin:set_angles(Angle3(0, look.y + 180, 0));
        mannequin:spawn();
        idx = idx + 1;
    end

    print("[anim] spawned " .. count .. " sent_mannequin");
end);
