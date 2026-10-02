local spawned = false;

hook.add("PlayerSpawned", "spawn_crate", function(handle)
    if spawned then
        return;
    end

    local ply = ents.get_by_index(handle:index());

    if ply == nil then
        return;
    end

    spawned = true;
    local look = ply:get_angles();
    local yaw = math.rad(look.y);
    local forward = Vector3(math.cos(yaw), math.sin(yaw), 0);
    local crate = ents.create("sent_crate");

    if crate == nil then
        print("[gltf] failed to create sent_crate");

        return;
    end

    crate:set_pos(ply:get_pos() + forward * 3);
    crate:set_angles(Angle3(0, look.y, 0));
    crate:spawn();
    print("[gltf] spawned sent_crate");
end);
