hook.add("Initialize", "scale_server", function()
    local scale = 1 / 24
    local unit = 24
    local half = unit * 6

    engine.set_map_scale(scale)
    --engine.set_voxel_scale(scale)
end)
