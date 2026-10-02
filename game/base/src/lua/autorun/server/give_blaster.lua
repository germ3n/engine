hook.add("PlayerSpawned", "give_blaster", function(handle)
    local ply = ents.get_by_index(handle:index());

    if ply == nil then
        return;
    end

    local blaster = ents.create("sent_blaster");
    blaster:set_owner(ply);
    blaster:spawn();
end);
