if gui then
local frame = gui.create("Panel");
frame:set_pos(100, 100);
frame:set_size(200, 200);
frame:set_paint(function(self, x, y, w, h)
    surface.draw_rect(x, y, w, h, 55, 55, 55, 100);
end);

local bar = gui.create("Panel");
bar:set_parent(frame);
bar:set_pos(0, 0);
bar:set_size(200, 25);
bar:set_paint(function(self, x, y, w, h)
    surface.draw_rect(x, y, w, h, 55, 55, 55, 100);
end);

local title = gui.create("Label");
title:set_parent(frame);
title:set_pos(0, 0);
title:set_size(200, 25);
title:set_text("Menu");
end

hook.add("PlayerSpawned", "test", function(id, pos)
    print("test plyspawned");
end);
