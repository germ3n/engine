local ffi = require("ffi")

ffi.cdef[[
    typedef struct {
        float x, y, z;
    } Vector3;
]]

local Vector3Ctor

local Vector3Meta = {
    __add = function(a, b)
        return Vector3Ctor(a.x + b.x, a.y + b.y, a.z + b.z)
    end,

    __sub = function(a, b)
        return Vector3Ctor(a.x - b.x, a.y - b.y, a.z - b.z)
    end,

    __mul = function(a, b)
        if type(a) == "number" then
            return Vector3Ctor(a * b.x, a * b.y, a * b.z)
        elseif type(b) == "number" then
            return Vector3Ctor(a.x * b, a.y * b, a.z * b)
        end
        return Vector3Ctor(a.x * b.x, a.y * b.y, a.z * b.z)
    end,

    __div = function(a, b)
        if type(b) == "number" then
            local inv = 1.0 / b
            return Vector3Ctor(a.x * inv, a.y * inv, a.z * inv)
        end
        return Vector3Ctor(a.x / b.x, a.y / b.y, a.z / b.z)
    end,

    __unm = function(a)
        return Vector3Ctor(-a.x, -a.y, -a.z)
    end,

    __eq = function(a, b)
        return a.x == b.x and a.y == b.y and a.z == b.z
    end,

    __tostring = function(a)
        return string.format("Vector3(%.4f, %.4f, %.4f)", a.x, a.y, a.z)
    end,
}

Vector3Meta.__index = {
    add_inplace = function(self, other)
        self.x = self.x + other.x
        self.y = self.y + other.y
        self.z = self.z + other.z
        return self
    end,

    sub_inplace = function(self, other)
        self.x = self.x - other.x
        self.y = self.y - other.y
        self.z = self.z - other.z
        return self
    end,

    mul_inplace = function(self, val)
        if type(val) == "number" then
            self.x = self.x * val
            self.y = self.y * val
            self.z = self.z * val
        else
            self.x = self.x * val.x
            self.y = self.y * val.y
            self.z = self.z * val.z
        end
        return self
    end,

    dot = function(self, other)
        return self.x * other.x + self.y * other.y + self.z * other.z
    end,

    cross = function(self, other)
        return Vector3Ctor(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x
        )
    end,

    len_sq = function(self)
        return self.x * self.x + self.y * self.y + self.z * self.z
    end,

    len = function(self)
        return math.sqrt(self.x * self.x + self.y * self.y + self.z * self.z)
    end,

    normalize = function(self)
        local len = self:len()
        if len > 0 then
            local inv = 1.0 / len
            return Vector3Ctor(self.x * inv, self.y * inv, self.z * inv)
        end
        return Vector3Ctor(self.x, self.y, self.z)
    end,

    normalize_inplace = function(self)
        local len = self:len()
        if len > 0 then
            local inv = 1.0 / len
            self.x = self.x * inv
            self.y = self.y * inv
            self.z = self.z * inv
        end
        return self
    end,

    sum_all = function(vectors)
        local out = Vector3Ctor(0, 0, 0)
        for idx = 1, #vectors do
            out:add_inplace(vectors[idx])
        end
        return out
    end,
}

Vector3Ctor = ffi.metatype("Vector3", Vector3Meta)

local function create_vec3(x, y, z)
    return Vector3Ctor(x or 0, y or 0, z or 0)
end

local Vector3Module = setmetatable({}, {
    __call = function(_, x, y, z)
        return create_vec3(x, y, z)
    end,
    __index = Vector3Meta.__index,
})

return {
    ctor = create_vec3,
    module = Vector3Module,
}