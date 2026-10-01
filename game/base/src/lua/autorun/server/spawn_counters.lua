hook.add("Initialize", "spawn_counters", function()
    local counter = ents.create("sent_counter");
    counter:set_pos(Vector3(0, 0, 64));
    counter:spawn();

    local fast = ents.create("sent_counter_fast");
    fast:set_pos(Vector3(64, 0, 64));
    fast:spawn();
end);
