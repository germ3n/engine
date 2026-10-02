function ENT:on_spawn()
    self:set_model("models/test.gltf");
    self:set_sequence("spin");
    self:enable_physics();
end
