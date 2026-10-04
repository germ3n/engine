function WEAPON:initialize()
    self:set_networked("ammo", self.clip_size, true);
    self:set_networked("next_fire", -1, true);
    self:set_model(self.model);
    self:set_sequence("wpn_val_draw");
    self._pose = "draw";

    self:log("initialized");
end
