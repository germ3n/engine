function ENT:on_networked_changed(key, old, value)
    self:log("from server " .. key .. " " .. tostring(old) .. " -> " .. tostring(value));
end
