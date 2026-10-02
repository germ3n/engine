--[=[document
kind = "class",
name = "Vector3",
realm = "shared",
summary = "Position or direction of three floats. Omitted constructor components are 0.",
params = {
    x = { ty = "number", desc = "X component.", optional = true },
    y = { ty = "number", desc = "Y component.", optional = true },
    z = { ty = "number", desc = "Z component.", optional = true },
},
returns = { ty = "Vector3", desc = "The new vector." },
example = "local pos = Vector3(0, 0, 64)",
note = "Operators: + - * / unary minus, ==, and tostring. Multiplication and division take a number or another Vector3.",
]=]
--[=[document
parent = "Vector3",
name = "x",
kind = "field",
realm = "shared",
summary = "X component.",
returns = { ty = "number", desc = "The x value." },
]=]
--[=[document
parent = "Vector3",
name = "y",
kind = "field",
realm = "shared",
summary = "Y component.",
returns = { ty = "number", desc = "The y value." },
]=]
--[=[document
parent = "Vector3",
name = "z",
kind = "field",
realm = "shared",
summary = "Z component.",
returns = { ty = "number", desc = "The z value." },
]=]
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
    --[=[document
    parent = "Vector3",
    name = "add_inplace",
    realm = "shared",
    summary = "Adds another vector into this one.",
    params = {
        other = { ty = "Vector3", desc = "Vector to add." },
    },
    returns = { ty = "Vector3", desc = "This vector." },
    ]=]
    add_inplace = function(self, other)
        self.x = self.x + other.x
        self.y = self.y + other.y
        self.z = self.z + other.z
        return self
    end,

    --[=[document
    parent = "Vector3",
    name = "sub_inplace",
    realm = "shared",
    summary = "Subtracts another vector from this one.",
    params = {
        other = { ty = "Vector3", desc = "Vector to subtract." },
    },
    returns = { ty = "Vector3", desc = "This vector." },
    ]=]
    sub_inplace = function(self, other)
        self.x = self.x - other.x
        self.y = self.y - other.y
        self.z = self.z - other.z
        return self
    end,

    --[=[document
    parent = "Vector3",
    name = "mul_inplace",
    realm = "shared",
    summary = "Multiplies this vector in place.",
    params = {
        val = { ty = "any", desc = "A number, or another Vector3 multiplied per component." },
    },
    returns = { ty = "Vector3", desc = "This vector." },
    ]=]
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

    --[=[document
    parent = "Vector3",
    name = "dot",
    realm = "shared",
    summary = "Dot product with another vector.",
    params = {
        other = { ty = "Vector3", desc = "The other vector." },
    },
    returns = { ty = "number", desc = "x*ox + y*oy + z*oz." },
    ]=]
    dot = function(self, other)
        return self.x * other.x + self.y * other.y + self.z * other.z
    end,

    --[=[document
    parent = "Vector3",
    name = "cross",
    realm = "shared",
    summary = "Cross product with another vector.",
    params = {
        other = { ty = "Vector3", desc = "The other vector." },
    },
    returns = { ty = "Vector3", desc = "A new perpendicular vector." },
    ]=]
    cross = function(self, other)
        return Vector3Ctor(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x
        )
    end,

    --[=[document
    parent = "Vector3",
    name = "len_sq",
    realm = "shared",
    summary = "Squared length.",
    returns = { ty = "number", desc = "x*x + y*y + z*z." },
    ]=]
    len_sq = function(self)
        return self.x * self.x + self.y * self.y + self.z * self.z
    end,

    --[=[document
    parent = "Vector3",
    name = "len",
    realm = "shared",
    summary = "Length.",
    returns = { ty = "number", desc = "Square root of the squared length." },
    ]=]
    len = function(self)
        return math.sqrt(self.x * self.x + self.y * self.y + self.z * self.z)
    end,

    --[=[document
    parent = "Vector3",
    name = "normalize",
    realm = "shared",
    summary = "Returns a unit vector. A zero vector is returned unchanged.",
    returns = { ty = "Vector3", desc = "A new vector." },
    ]=]
    normalize = function(self)
        local len = self:len()
        if len > 0 then
            local inv = 1.0 / len
            return Vector3Ctor(self.x * inv, self.y * inv, self.z * inv)
        end
        return Vector3Ctor(self.x, self.y, self.z)
    end,

    --[=[document
    parent = "Vector3",
    name = "normalize_inplace",
    realm = "shared",
    summary = "Scales this vector to unit length. A zero vector is left unchanged.",
    returns = { ty = "Vector3", desc = "This vector." },
    ]=]
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

    --[=[document
    parent = "Vector3",
    name = "sum_all",
    kind = "function",
    realm = "shared",
    summary = "Adds every vector in a list.",
    params = {
        vectors = { ty = "table", desc = "Array of Vector3 values." },
    },
    returns = { ty = "Vector3", desc = "The sum, or the zero vector when the list is empty." },
    example = "local sum = Vector3.sum_all({ Vector3(1, 0, 0), Vector3(0, 2, 0) })",
    ]=]
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
