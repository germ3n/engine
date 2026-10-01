function ENT:think()
    self.base_class.think(self);

    if self:get_count() >= self.max_count then
        self:log("reached " .. self.max_count .. ", removing");
        self:remove();
    end
end
