WEAPON.primary = {
    damage = 0,
    shoot_sound = nil,
    shoot_sequence = nil,
};

WEAPON.secondary = {
    damage = 0,
    shoot_sound = nil,
    shoot_sequence = nil,
};

WEAPON.idle_sequence = nil;
WEAPON.walk_sequence = nil;
WEAPON.sprint_sequence = nil;

function WEAPON:primary_attack()
end

function WEAPON:secondary_attack()
end

function WEAPON:primary_reload()
end

function WEAPON:secondary_reload()
end

function WEAPON:calc_viewmodel_view()
end

function WEAPON:calc_fov()
end
