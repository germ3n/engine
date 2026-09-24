hook.add("MenuPaint", "menu", function()
    surface.draw_rect(100, 100, 200, 200, 55, 55, 55, 100);
    surface.draw_rect(100, 100, 200, 25, 55, 55, 55, 100);
    surface.draw_text("default", "Menu", 100, 100, 16, 255, 255, 255, 255);
end);

hook.add("PlayerSpawned", "test", function(id, pos)
    print("test plyspawned");
end);

cvar.get("sv_gravity"):add_change_callback(function(new_value)
    print("callback", new_value)
end);

net.add_callback("Test", function(reader)
    local writer = net.writer(4); -- with 4 capacity, todo: maybe error when exceeding capacity
    print(tostring(writer))
    writer:write_f32(0);

    net.send("Test", writer);
    print(tostring(cvar.get("sv_gravity")), tostring(cvar.get("sv_gravity"):get_value_float()));

    cvar.get("sv_gravity"):set_value_float(600.0)
end);