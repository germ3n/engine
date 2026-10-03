--[=[document
kind = "library",
name = "gui",
realm = "client",
summary = "Retained panels drawn with surface. Clipping uses a scissor stack. A panel can cache into a render target.",
]=]
gui = {};

local stack = {};
local roots = {};
local html_list = {};
local focused_html = nil;
local html_blocking = false;
local held_keys = {};
local key_owner = nil;

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

--[=[document
kind = "class",
name = "Panel",
realm = "client",
summary = "A retained rectangle. Children paint inside it. set_paint replaces the default draw. A cached panel paints into a texture until invalidate.",
]=]
local Panel = {};
Panel.__index = Panel;

--[=[document
parent = "Panel",
name = "set_pos",
realm = "client",
summary = "Moves the panel and marks it dirty.",
params = {
    x = { ty = "number", desc = "Left edge in parent pixels." },
    y = { ty = "number", desc = "Top edge in parent pixels." },
},
]=]
function Panel:set_pos(x, y)
    self.x = x;
    self.y = y;
    self:invalidate();
end

--[=[document
parent = "Panel",
name = "set_size",
realm = "client",
summary = "Resizes the panel. A cached panel also resizes its render target.",
params = {
    w = { ty = "number", desc = "Width in pixels." },
    h = { ty = "number", desc = "Height in pixels." },
},
]=]
function Panel:set_size(w, h)
    self.w = w;
    self.h = h;

    if self.target then
        surface.update_target(self.target, math.max(1, math.floor(w)), math.max(1, math.floor(h)));
    end

    self:invalidate();
end

--[=[document
parent = "Panel",
name = "set_visible",
realm = "client",
summary = "Shows or hides the panel. A hidden panel is skipped by paint and by gui.hit.",
params = {
    visible = { ty = "boolean", desc = "False hides it." },
},
see_also = "gui.hit",
]=]
function Panel:set_visible(visible)
    self.visible = visible and true or false;
end

--[=[document
parent = "Panel",
name = "detach",
realm = "client",
summary = "Removes the panel from its parent, or from the root list when it has no parent. The panel stays alive.",
]=]
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

--[=[document
parent = "Panel",
name = "set_parent",
realm = "client",
summary = "Detaches the panel and adds it as the last child of parent.",
params = {
    parent = { ty = "Panel", desc = "The new parent." },
},
see_also = "Panel:detach",
]=]
function Panel:set_parent(parent)
    self:detach();
    self.parent = parent;
    parent.children[#parent.children + 1] = self;
    parent:invalidate();
end

--[=[document
parent = "Panel",
name = "set_paint",
realm = "client",
summary = "Replaces the panel's own draw. Children still paint after the callback.",
params = {
    callback = { ty = "function", desc = "function(panel, x, y, w, h)" },
},
]=]
function Panel:set_paint(callback)
    self.paint_fn = callback;
    self:invalidate();
end

--[=[document
parent = "Panel",
name = "set_cached",
realm = "client",
summary = "When true, the panel paints into a render target and later frames draw that texture until invalidate.",
params = {
    cached = { ty = "boolean", desc = "True turns caching on." },
},
see_also = "Panel:invalidate",
]=]
function Panel:set_cached(cached)
    self.cached = cached and true or false;

    if self.cached and not self.target then
        self.target = surface.create_target(math.max(1, math.floor(self.w)), math.max(1, math.floor(self.h)));
    end

    self:invalidate();
end

--[=[document
parent = "Panel",
name = "invalidate",
realm = "client",
summary = "Marks this panel and its parents dirty so a cached panel paints again.",
]=]
function Panel:invalidate()
    self.dirty = true;
    local parent = self.parent;

    while parent do
        parent.dirty = true;
        parent = parent.parent;
    end
end

--[=[document
parent = "Panel",
name = "set_text",
realm = "client",
summary = "Sets the text string and marks the panel dirty.",
params = {
    text = { ty = "string", desc = "The new text." },
},
]=]
function Panel:set_text(text)
    self.text = text;
    self:invalidate();
end

--[=[document
parent = "Panel",
name = "on_click",
realm = "client",
summary = "Sets the left-click callback. It is called as callback(panel) when gui.hit finds this panel.",
params = {
    callback = { ty = "function", desc = "function(panel)" },
},
see_also = "gui.hit",
]=]
function Panel:on_click(callback)
    self.click = callback;
end

--[=[document
parent = "Panel",
name = "remove",
realm = "client",
summary = "Hides the panel, detaches it, and frees its cache texture.",
]=]
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

--[=[document
kind = "class",
name = "Label",
realm = "client",
summary = "A Panel that draws its text. scale is the pixel height. r, g, b, and a are the color, 0 to 255.",
see_also = "Panel:set_text",
]=]
local Label = setmetatable({}, { __index = Panel });
Label.__index = Label;

function Label:draw_self(x, y, w, h)
    surface.draw_text("default", self.text, x, y, self.scale, self.r, self.g, self.b, self.a);
end

--[=[document
kind = "class",
name = "Button",
realm = "client",
summary = "A Panel that fills itself and draws its text. br, bg, bb, and ba are the fill, 0 to 255. A white wash is drawn while the cursor is inside it.",
see_also = "Panel:set_text, Panel:on_click",
]=]
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

local function origin(panel)
    local x = panel.x;
    local y = panel.y;
    local parent = panel.parent;

    while parent do
        x = x + parent.x;
        y = y + parent.y;
        parent = parent.parent;
    end

    return x, y;
end

local image_pipe = nil;

local function image_pipeline()
    if image_pipe then
        return image_pipe;
    end

    local shader = surface.create_shader([[
struct ScreenUniforms { resolution: vec4<f32> }
var<immediate> pc: ScreenUniforms;
@group(0) @binding(0) var image_tex: texture_2d<f32>;
@group(0) @binding(1) var image_samp: sampler;
struct VsIn {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
}
struct VsOut {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
}
@vertex
fn vs_main(vin: VsIn) -> VsOut {
    var unit = vin.position / pc.resolution.xy;
    var clip = unit * 2.0 - 1.0;
    clip.y = -clip.y;
    var vout: VsOut;
    vout.clip_position = vec4(clip, 0.0, 1.0);
    vout.uv = vin.uv;
    vout.color = vin.color;
    return vout;
}
@fragment
fn fs_main(vin: VsOut) -> @location(0) vec4<f32> {
    let sample = textureSample(image_tex, image_samp, vin.uv);
    return vec4(sample.rgb * vin.color.rgb, sample.a * vin.color.a);
}
]]);
    image_pipe = surface.create_pipeline(shader, "screen");

    return image_pipe;
end

local function is_text_key(name)
    if #name == 1 then
        return true;
    end

    return name == "space";
end

local function release_keys()
    local panel = key_owner or focused_html;
    local name = next(held_keys);

    while name do
        local nxt = next(held_keys, name);

        if not input.key_down(name) then
            if panel and panel.view then
                panel.view:key(name, false, false);
            end

            held_keys[name] = nil;
        end

        name = nxt;
    end

    if next(held_keys) == nil then
        key_owner = focused_html;
    end
end

--[=[document
kind = "class",
name = "Html",
realm = "client",
summary = "A Panel that shows a webview. The page is created on the first load. Mouse, wheel, and keys are forwarded while the cursor is over it.",
see_also = "webview.create, Panel",
]=]
local Html = setmetatable({}, { __index = Panel });
Html.__index = Html;

function Html:ensure()
    if self.view or self.view_failed then
        return;
    end

    local w = math.max(1, math.floor(self.w));
    local h = math.max(1, math.floor(self.h));
    self.view = webview.create(w, h);

    if not self.view then
        self.view_failed = true;

        return;
    end

    self.down = { false, false, false };
    html_list[#html_list + 1] = self;
end

--[=[document
parent = "Html",
name = "set_size",
realm = "client",
summary = "Resizes the panel and the webview.",
params = {
    w = { ty = "number", desc = "Width in pixels." },
    h = { ty = "number", desc = "Height in pixels." },
},
see_also = "Panel:set_size",
]=]
function Html:set_size(w, h)
    Panel.set_size(self, w, h);

    if self.view then
        self.view:resize(math.max(1, math.floor(self.w)), math.max(1, math.floor(self.h)));
    end
end

--[=[document
parent = "Html",
name = "load_html",
realm = "client",
summary = "Creates the page if needed and loads an HTML document.",
params = {
    html = { ty = "string", desc = "Document source." },
},
see_also = "WebView:load_html",
]=]
function Html:load_html(html)
    self:ensure();

    if self.view then
        self.view:load_html(html);
    end
end

--[=[document
parent = "Html",
name = "load_url",
realm = "client",
summary = "Creates the page if needed and loads a URL.",
params = {
    url = { ty = "string", desc = "http or https URL." },
},
see_also = "WebView:load_url",
]=]
function Html:load_url(url)
    self:ensure();

    if self.view then
        self.view:load_url(url);
    end
end

--[=[document
parent = "Html",
name = "run_js",
realm = "client",
summary = "Creates the page if needed and runs JavaScript in it.",
params = {
    code = { ty = "string", desc = "Script source." },
},
see_also = "WebView:run_js",
]=]
function Html:run_js(code)
    self:ensure();

    if self.view then
        self.view:run_js(code);
    end
end

--[=[document
parent = "Html",
name = "on_message",
realm = "client",
summary = "Sets the callback for window.engine.post. The callback is called as callback(panel, text).",
params = {
    callback = { ty = "function", desc = "function(panel, text)" },
},
see_also = "WebView:on_message",
]=]
function Html:on_message(callback)
    self:ensure();

    if self.view then
        self.view:on_message(function(text)
            callback(self, text);
        end);
    end
end

function Html:draw_self(x, y, w, h)
    self:ensure();

    if not self.view then
        return;
    end

    local tex = self.view:texture();

    if tex ~= 0 then
        surface.draw_rect(x, y, w, h, 255, 255, 255, 255, tex, image_pipeline(), surface.sampler("clamp"));
    end
end

function Html:feed(hovered, mx, my)
    self:ensure();

    if not self.view then
        return;
    end

    local ox, oy = origin(self);

    if hovered then
        self.view:mouse_move(mx - ox, my - oy);
    end

    local idx = 1;

    while idx <= 3 do
        local down = hovered and input.mouse_down(idx);
        local was = self.down[idx];

        if down and not was then
            focused_html = self;
            self.view:focus(true);
            self.view:mouse_button(idx, true);
        elseif was and not down then
            self.view:mouse_button(idx, false);
        end

        self.down[idx] = down and true or false;
        idx = idx + 1;
    end

    if hovered then
        local wx, wy = input.wheel();

        if wx ~= 0 or wy ~= 0 then
            self.view:mouse_wheel(wx, wy);
        end
    end
end

--[=[document
parent = "Html",
name = "remove",
realm = "client",
summary = "Closes the webview, then removes the panel.",
see_also = "Panel:remove, WebView:remove",
]=]
function Html:remove()
    if self.view then
        local name = next(held_keys);

        while name do
            local nxt = next(held_keys, name);
            self.view:key(name, false, false);
            held_keys[name] = nil;
            name = nxt;
        end

        self.view:focus(false);
        self.view:remove();
        self.view = nil;
    end

    local idx = 1;

    while idx <= #html_list do
        if html_list[idx] == self then
            table.remove(html_list, idx);
        else
            idx = idx + 1;
        end
    end

    if focused_html == self then
        focused_html = nil;
    end

    if key_owner == self then
        key_owner = nil;
    end

    Panel.remove(self);
end

local function over_html()
    if input.captured() then
        return nil;
    end

    local mx, my = input.cursor();
    local hit = gui.hit(mx, my);

    if hit and hit.kind == "Html" then
        return hit;
    end
end

local function feed_html()
    local hovered = over_html();

    if hovered then
        input.block_look(true);
        html_blocking = true;
    elseif html_blocking then
        input.block_look(false);
        html_blocking = false;
    end

    local mx, my = input.cursor();
    local idx = 1;

    while idx <= #html_list do
        local panel = html_list[idx];

        if not panel.alive then
            table.remove(html_list, idx);
        else
            panel:feed(panel == hovered, mx, my);
            idx = idx + 1;
        end
    end

    release_keys();
end

local kinds = {
    Panel = Panel,
    Label = Label,
    Button = Button,
    Html = Html,
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
summary = "Creates a Panel, Label, Button, or Html and adds it to the root list until set_parent.",
params = {
    kind = { ty = "string", desc = "Panel, Label, Button, or Html." },
},
returns = { ty = "panel", desc = "The new panel." },
example = "local frame = gui.create(\"Panel\")",
see_also = "gui.hit",
]=]
function gui.create(kind)
    local class = kinds[kind] or Panel;
    local panel = setmetatable(blank(kind or "Panel"), class);
    roots[#roots + 1] = panel;

    if panel.kind == "Html" then
        panel:ensure();
    end

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
    feed_html();
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
    local hit = gui.hit(x, y);

    if hit and hit.kind == "Html" then
        if input.captured() then
            return nil;
        end

        return true;
    end

    if focused_html and button == 1 then
        if focused_html.view then
            focused_html.view:focus(false);
        end

        focused_html = nil;
    end

    if button ~= 1 then
        return nil;
    end

    if not hit then
        return nil;
    end

    if hit.click then
        hit.click(hit);
    end

    return true;
end);

hook.add("GuiKeyPressed", "webview", function(name, repeated)
    local panel = focused_html;

    if not panel or not panel.view or not panel.alive then
        return nil;
    end

    if input.captured() then
        return nil;
    end

    if name == "`" then
        return nil;
    end

    local command = input.control() or input.super() or input.alt();

    if name == "escape" and not repeated then
        panel.view:key(name, true, false);
        held_keys[name] = true;
        key_owner = panel;
        panel.view:focus(false);
        focused_html = nil;

        return true;
    end

    if command or not is_text_key(name) then
        panel.view:key(name, true, repeated and true or false);
        held_keys[name] = true;
        key_owner = panel;
    end

    return true;
end);

hook.add("GuiText", "webview", function(text)
    local panel = focused_html;

    if not panel or not panel.view or not panel.alive then
        return nil;
    end

    if input.captured() or input.control() or input.super() or input.alt() then
        return nil;
    end

    if text == "\n" or text == "\r" or text == "\t" then
        return nil;
    end

    panel.view:text(text);

    return true;
end);
