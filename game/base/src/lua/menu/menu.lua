hook.add("MenuPaint", "menu", function()
    surface.draw_rect(100, 100, 200, 200, 55, 55, 55, 100);
    surface.draw_rect(100, 100, 200, 25, 55, 55, 55, 100);
    surface.draw_text("default", "Menu", 100, 100, 16, 255, 255, 255, 255);
end);

hook.add("PlayerSpawned", "test", function(id, pos)
    print("test plyspawned");
end);

net.add_callback("Test", function(reader)
    local writer = net.writer();
    print(tostring(writer))
    writer:write_f32(0);

    net.send("Test", writer);
end);