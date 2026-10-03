--[=[document
kind = "library",
name = "gui",
realm = "client",
summary = "Retained panels drawn with surface. Clipping uses a scissor stack. A panel can cache into a render target.",
]=]
gui = {};

local stack = {};
local roots = {};

local function copy_stack()
    local saved = {};
    local idx = 1;

    while idx <= #stack do
        local rect = stack[idx];
        saved[idx] = { rect[1], rect[2], rect[3], rect[4] };
        idx = idx + 1;
    end

    return saved;
end

local function apply_top()
    local top = stack[#stack];

    if top then
        surface.set_scissor(top[1], top[2], top[3], top[4]);
    else
        surface.set_scissor();
    end
end

--[=[document
parent = "surface",
name = "push_scissor",
realm = "client",
summary = "Intersects a rect with the current scissor and pushes it. Coordinates match surface.draw_rect.",
params = {
    x = { ty = "number", desc = "Left edge in pixels." },
    y = { ty = "number", desc = "Top edge in pixels." },
    w = { ty = "number", desc = "Width in pixels." },
    h = { ty = "number", desc = "Height in pixels." },
},
returns = { ty = "nil", desc = "" },
example = "surface.push_scissor(10, 10, 80, 24)",
see_also = "surface.pop_scissor",
]=]
function surface.push_scissor(x, y, w, h)
    local top = stack[#stack];
    local rect = { x, y, w, h };

    if top then
        local x1 = math.max(top[1], x);
        local y1 = math.max(top[2], y);
        local x2 = math.min(top[1] + top[3], x + w);
        local y2 = math.min(top[2] + top[4], y + h);
        rect = { x1, y1, math.max(0, x2 - x1), math.max(0, y2 - y1) };
    end

    stack[#stack + 1] = rect;
    surface.set_scissor(rect[1], rect[2], rect[3], rect[4]);
end

--[=[document
parent = "surface",
name = "pop_scissor",
realm = "client",
summary = "Pops the scissor pushed by surface.push_scissor.",
returns = { ty = "nil", desc = "" },
example = "surface.pop_scissor()",
see_also = "surface.push_scissor",
]=]
function surface.pop_scissor()
    if #stack > 0 then
        stack[#stack] = nil;
    end

    apply_top();
end

local function save_scissor()
    return copy_stack();
end

local function restore_scissor(saved)
    stack = saved;
    apply_top();
end

local Panel = {};
Panel.__index = Panel;

function Panel:set_pos(x, y)
    self.x = x;
    self.y = y;
    self:invalidate();
end

function Panel:set_size(w, h)
    self.w = w;
    self.h = h;

    if self.target then
        surface.update_target(self.target, math.max(1, math.floor(w)), math.max(1, math.floor(h)));
    end

    self:invalidate();
end

function Panel:set_visible(visible)
    self.visible = visible and true or false;
end

function Panel:detach()
    if self.parent then
        local kids = self.parent.children;
        local idx = 1;

        while idx <= #kids do
            if kids[idx] == self then
                table.remove(kids, idx);
            else
                idx = idx + 1;
            end
        end

        self.parent = nil;

        return;
    end

    local idx = 1;

    while idx <= #roots do
        if roots[idx] == self then
            table.remove(roots, idx);
        else
            idx = idx + 1;
        end
    end
end

function Panel:set_parent(parent)
    self:detach();
    self.parent = parent;
    parent.children[#parent.children + 1] = self;
    parent:invalidate();
end

function Panel:set_paint(callback)
    self.paint_fn = callback;
    self:invalidate();
end

function Panel:set_cached(cached)
    self.cached = cached and true or false;

    if self.cached and not self.target then
        self.target = surface.create_target(math.max(1, math.floor(self.w)), math.max(1, math.floor(self.h)));
    end

    self:invalidate();
end

function Panel:invalidate()
    self.dirty = true;
    local parent = self.parent;

    while parent do
        parent.dirty = true;
        parent = parent.parent;
    end
end

function Panel:set_text(text)
    self.text = text;
    self:invalidate();
end

function Panel:on_click(callback)
    self.click = callback;
end

function Panel:remove()
    self.alive = false;
    self.visible = false;
    self:detach();

    if self.target then
        surface.free(self.target);
        self.target = nil;
    end
end

function Panel:draw_self(x, y, w, h)
end

function Panel:draw_hover(x, y, w, h)
end

function Panel:draw_contents(x, y, host)
    if self.paint_fn then
        self.paint_fn(self, x, y, self.w, self.h);
    else
        self:draw_self(x, y, self.w, self.h);
    end

    local idx = 1;

    while idx <= #self.children do
        self.children[idx]:paint_at(x, y, host);
        idx = idx + 1;
    end
end

function Panel:paint_at(ox, oy, host)
    if not self.visible or not self.alive then
        return;
    end

    host = host or 0;
    local x = ox + self.x;
    local y = oy + self.y;
    local w = self.w;
    local h = self.h;

    if self.cached and self.target and not self.dirty then
        surface.draw_rect(x, y, w, h, 255, 255, 255, 255, self.target);

        return;
    end

    if self.cached and self.target and self.dirty then
        local saved = save_scissor();
        stack = {};
        surface.set_target(self.target);
        surface.set_scissor();
        surface.push_scissor(0, 0, w, h);
        self:draw_contents(0, 0, self.target);
        surface.pop_scissor();
        surface.set_target(host);
        restore_scissor(saved);
        self.dirty = false;
        surface.draw_rect(x, y, w, h, 255, 255, 255, 255, self.target);

        return;
    end

    surface.push_scissor(x, y, w, h);
    self:draw_contents(x, y, host);
    surface.pop_scissor();
end

function Panel:paint_hover(ox, oy)
    if not self.visible or not self.alive then
        return;
    end

    local x = ox + self.x;
    local y = oy + self.y;
    surface.push_scissor(x, y, self.w, self.h);
    self:draw_hover(x, y, self.w, self.h);
    local idx = 1;

    while idx <= #self.children do
        self.children[idx]:paint_hover(x, y);
        idx = idx + 1;
    end

    surface.pop_scissor();
end

function Panel:hit(px, py, ox, oy)
    if not self.visible or not self.alive then
        return nil;
    end

    local x = ox + self.x;
    local y = oy + self.y;

    if px < x or py < y or px >= x + self.w or py >= y + self.h then
        return nil;
    end

    local idx = #self.children;

    while idx >= 1 do
        local found = self.children[idx]:hit(px, py, x, y);

        if found then
            return found;
        end

        idx = idx - 1;
    end

    return self;
end

local Label = setmetatable({}, { __index = Panel });
Label.__index = Label;

function Label:draw_self(x, y, w, h)
    surface.draw_text("default", self.text, x, y, self.scale, self.r, self.g, self.b, self.a);
end

local Button = setmetatable({}, { __index = Panel });
Button.__index = Button;

function Button:draw_self(x, y, w, h)
    surface.draw_rect(x, y, w, h, self.br, self.bg, self.bb, self.ba);
    surface.draw_text("default", self.text, x + 8, y + 4, self.scale, self.r, self.g, self.b, self.a);
end

function Button:draw_hover(x, y, w, h)
    local mx, my = input.cursor();

    if mx >= x and my >= y and mx < x + w and my < y + h then
        surface.draw_rect(x, y, w, h, 255, 255, 255, 40);
    end
end

local kinds = {
    Panel = Panel,
    Label = Label,
    Button = Button,
};

local function blank(kind)
    return {
        x = 0,
        y = 0,
        w = 100,
        h = 100,
        visible = true,
        alive = true,
        dirty = true,
        cached = false,
        children = {},
        text = "",
        scale = 16,
        r = 255,
        g = 255,
        b = 255,
        a = 255,
        br = 40,
        bg = 40,
        bb = 40,
        ba = 255,
        kind = kind,
    };
end

--[=[document
parent = "gui",
name = "create",
realm = "client",
summary = "Creates a Panel, Label, or Button and adds it to the root list until set_parent.",
params = {
    kind = { ty = "string", desc = "Panel, Label, or Button." },
},
returns = { ty = "panel", desc = "The new panel." },
example = "local frame = gui.create(\"Panel\")",
see_also = "gui.hit",
]=]
function gui.create(kind)
    local class = kinds[kind] or Panel;
    local panel = setmetatable(blank(kind or "Panel"), class);
    roots[#roots + 1] = panel;

    return panel;
end

--[=[document
parent = "gui",
name = "hit",
realm = "client",
summary = "Returns the top-most visible panel under a point, or nil.",
params = {
    x = { ty = "number", desc = "Cursor x." },
    y = { ty = "number", desc = "Cursor y." },
},
returns = { ty = "panel", desc = "The hit panel, or nil." },
example = "local panel = gui.hit(12, 40)",
see_also = "gui.create",
]=]
function gui.hit(x, y)
    local idx = #roots;

    while idx >= 1 do
        local found = roots[idx]:hit(x, y, 0, 0);

        if found then
            return found;
        end

        idx = idx - 1;
    end
end

hook.add("MenuPaint", "gui", function()
    local idx = 1;

    while idx <= #roots do
        roots[idx]:paint_at(0, 0, 0);
        idx = idx + 1;
    end

    idx = 1;

    while idx <= #roots do
        roots[idx]:paint_hover(0, 0);
        idx = idx + 1;
    end
end);

hook.add("GuiMousePressed", "gui", function(button, x, y)
    if button ~= 1 then
        return nil;
    end

    local hit = gui.hit(x, y);

    if not hit then
        return nil;
    end

    if hit.click then
        hit.click(hit);
    end

    return true;
end);
