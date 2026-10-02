--[=[document
kind = "class",
name = "Angle3",
realm = "shared",
summary = "Pitch, yaw, and roll in degrees. Omitted constructor components are 0.",
params = {
    p = { ty = "number", desc = "Pitch.", optional = true },
    y = { ty = "number", desc = "Yaw.", optional = true },
    r = { ty = "number", desc = "Roll.", optional = true },
},
returns = { ty = "Angle3", desc = "The new angle." },
example = "local ang = Angle3(0, 90, 0)",
note = "Operators match Vector3: + - * / unary minus, ==, and tostring. Multiplication and division take a number or another Angle3. normalize wraps each component into (-180, 180].",
]=]
--[=[document
parent = "Angle3",
name = "p",
kind = "field",
realm = "shared",
summary = "Pitch, in degrees.",
returns = { ty = "number", desc = "The pitch." },
]=]
--[=[document
parent = "Angle3",
name = "y",
kind = "field",
realm = "shared",
summary = "Yaw, in degrees.",
returns = { ty = "number", desc = "The yaw." },
]=]
--[=[document
parent = "Angle3",
name = "r",
kind = "field",
realm = "shared",
summary = "Roll, in degrees.",
returns = { ty = "number", desc = "The roll." },
]=]
local ffi = require("ffi")

ffi.cdef[[
    typedef struct {
        float p, y, r;
    } Angle3;
]]

local Angle3Ctor

local Angle3Meta = {
    __add = function(a, b)
        return Angle3Ctor(a.p + b.p, a.y + b.y, a.r + b.r)
    end,

    __sub = function(a, b)
        return Angle3Ctor(a.p - b.p, a.y - b.y, a.r - b.r)
    end,

    __mul = function(a, b)
        if type(a) == "number" then
            return Angle3Ctor(a * b.p, a * b.y, a * b.r)
        elseif type(b) == "number" then
            return Angle3Ctor(a.p * b, a.y * b, a.r * b)
        end
        return Angle3Ctor(a.p * b.p, a.y * b.y, a.r * b.r)
    end,

    __div = function(a, b)
        if type(b) == "number" then
            local inv = 1.0 / b
            return Angle3Ctor(a.p * inv, a.y * inv, a.r * inv)
        end
        return Angle3Ctor(a.p / b.p, a.y / b.y, a.r / b.r)
    end,

    __unm = function(a)
        return Angle3Ctor(-a.p, -a.y, -a.r)
    end,

    __eq = function(a, b)
        return a.p == b.p and a.y == b.y and a.r == b.r
    end,

    __tostring = function(a)
        return string.format("Angle3(%.4f, %.4f, %.4f)", a.p, a.y, a.r)
    end,
}

Angle3Meta.__index = {
    --[=[document
    parent = "Angle3",
    name = "add_inplace",
    realm = "shared",
    summary = "Adds another angle into this one.",
    params = {
        other = { ty = "Angle3", desc = "Angle to add." },
    },
    returns = { ty = "Angle3", desc = "This angle." },
    ]=]
    add_inplace = function(self, other)
        self.p = self.p + other.p
        self.y = self.y + other.y
        self.r = self.r + other.r
        return self
    end,

    --[=[document
    parent = "Angle3",
    name = "sub_inplace",
    realm = "shared",
    summary = "Subtracts another angle from this one.",
    params = {
        other = { ty = "Angle3", desc = "Angle to subtract." },
    },
    returns = { ty = "Angle3", desc = "This angle." },
    ]=]
    sub_inplace = function(self, other)
        self.p = self.p - other.p
        self.y = self.y - other.y
        self.r = self.r - other.r
        return self
    end,

    --[=[document
    parent = "Angle3",
    name = "mul_inplace",
    realm = "shared",
    summary = "Multiplies this angle in place.",
    params = {
        val = { ty = "any", desc = "A number, or another Angle3 multiplied per component." },
    },
    returns = { ty = "Angle3", desc = "This angle." },
    ]=]
    mul_inplace = function(self, val)
        if type(val) == "number" then
            self.p = self.p * val
            self.y = self.y * val
            self.r = self.r * val
        else
            self.p = self.p * val.p
            self.y = self.y * val.y
            self.r = self.r * val.r
        end
        return self
    end,

    --[=[document
    parent = "Angle3",
    name = "normalize",
    realm = "shared",
    summary = "Wraps each component into the range (-180, 180].",
    returns = { ty = "Angle3", desc = "A new angle." },
    ]=]
    normalize = function(self)
        return Angle3Ctor(
            (self.p + 180.0) % 360.0 - 180.0,
            (self.y + 180.0) % 360.0 - 180.0,
            (self.r + 180.0) % 360.0 - 180.0
        )
    end,

    --[=[document
    parent = "Angle3",
    name = "normalize_inplace",
    realm = "shared",
    summary = "Wraps each component of this angle into the range (-180, 180].",
    returns = { ty = "Angle3", desc = "This angle." },
    ]=]
    normalize_inplace = function(self)
        self.p = (self.p + 180.0) % 360.0 - 180.0
        self.y = (self.y + 180.0) % 360.0 - 180.0
        self.r = (self.r + 180.0) % 360.0 - 180.0
        return self
    end,

    --[=[document
    parent = "Angle3",
    name = "sum_all",
    kind = "function",
    realm = "shared",
    summary = "Adds every angle in a list.",
    params = {
        angles = { ty = "table", desc = "Array of Angle3 values." },
    },
    returns = { ty = "Angle3", desc = "The sum, or the zero angle when the list is empty." },
    example = "local sum = Angle3.sum_all({ Angle3(0, 90, 0), Angle3(10, 0, 0) })",
    ]=]
    sum_all = function(angles)
        local out = Angle3Ctor(0, 0, 0)
        for idx = 1, #angles do
            out:add_inplace(angles[idx])
        end
        return out
    end,
}

Angle3Ctor = ffi.metatype("Angle3", Angle3Meta)

local function create_ang3(p, y, r)
    return Angle3Ctor(p or 0, y or 0, r or 0)
end

local Angle3Module = setmetatable({}, {
    __call = function(_, p, y, r)
        return create_ang3(p, y, r)
    end,
    __index = Angle3Meta.__index,
})

return {
    ctor = create_ang3,
    module = Angle3Module,
}
