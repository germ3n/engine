--[=[document
kind = "class",
name = "UserCmd",
realm = "shared",
summary = "One player command for move simulation and predicted_think.",
params = {
    tick = { ty = "number", desc = "Simulation tick.", optional = true },
    buttons = { ty = "number", desc = "Button bitfield.", optional = true },
    wish_x = { ty = "number", desc = "Wish X.", optional = true },
    wish_y = { ty = "number", desc = "Wish Y.", optional = true },
    wish_z = { ty = "number", desc = "Wish Z.", optional = true },
    view_p = { ty = "number", desc = "View pitch.", optional = true },
    view_y = { ty = "number", desc = "View yaw.", optional = true },
    view_r = { ty = "number", desc = "View roll.", optional = true },
    first_time_predicted = { ty = "boolean", desc = "True on the first simulation of this command.", optional = true },
},
returns = { ty = "UserCmd", desc = "The new command." },
example = "hook.add(\"Move\", \"mod\", function(ply, cmd)\n    print(cmd.tick, cmd.buttons, cmd.wish.x, cmd.first_time_predicted)\nend)",
see_also = "engine.first_time_predicted, hook.PreMove, hook.Move, hook.PostMove",
]=]
--[=[document
parent = "UserCmd",
name = "tick",
kind = "field",
realm = "shared",
summary = "Simulation tick this command belongs to.",
returns = { ty = "number", desc = "Tick count." },
]=]
--[=[document
parent = "UserCmd",
name = "buttons",
kind = "field",
realm = "shared",
summary = "Pressed input buttons bitfield.",
returns = { ty = "number", desc = "Button bits." },
]=]
--[=[document
parent = "UserCmd",
name = "wish",
kind = "field",
realm = "shared",
summary = "Wish movement direction.",
returns = { ty = "Vector3", desc = "Wish vector fields x, y, z." },
]=]
--[=[document
parent = "UserCmd",
name = "view",
kind = "field",
realm = "shared",
summary = "View angles for this command.",
returns = { ty = "Angle3", desc = "View angles." },
]=]
--[=[document
parent = "UserCmd",
name = "first_time_predicted",
kind = "field",
realm = "shared",
summary = "True on the first simulation of this command. False while the client reconciles a saved command.",
returns = { ty = "boolean", desc = "First-prediction flag." },
see_also = "engine.first_time_predicted",
]=]
local ffi = require("ffi")

ffi.cdef[[
    typedef struct {
        double tick;
        double buttons;
        struct {
            double x, y, z;
        } wish;
        Angle3 view;
        bool first_time_predicted;
    } UserCmd;
]]

local UserCmdCtor = ffi.metatype("UserCmd", {})

local function create(tick, buttons, wish_x, wish_y, wish_z, view_p, view_y, view_r, first_time)
    local cmd = UserCmdCtor()
    cmd.tick = tick or 0
    cmd.buttons = buttons or 0
    cmd.wish.x = wish_x or 0
    cmd.wish.y = wish_y or 0
    cmd.wish.z = wish_z or 0
    cmd.view.p = view_p or 0
    cmd.view.y = view_y or 0
    cmd.view.r = view_r or 0
    cmd.first_time_predicted = first_time and true or false
    return cmd
end

local UserCmd = setmetatable({}, {
    __call = function(_, tick, buttons, wish_x, wish_y, wish_z, view_p, view_y, view_r, first_time)
        return create(tick, buttons, wish_x, wish_y, wish_z, view_p, view_y, view_r, first_time)
    end,
})

return {
    ctor = create,
    module = UserCmd,
}
