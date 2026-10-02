function ENT:initialize()
    self:set_networked("ammo", self.clip_size, true);
    self:set_networked("next_fire", 0, true);

    self:log("initialized");
end
