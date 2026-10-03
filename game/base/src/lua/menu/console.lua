if not gui or not console or not surface or not input then
    return;
end

local open = false;
local input_text = "";
local lines = {};
local history = {};
local history_at = 0;
local draft = "";
local scroll = 0;
local matches = {};

local function pop_char(text)
    local idx = #text;

    if idx == 0 then
        return text;
    end

    while idx > 1 do
        local byte = text:byte(idx);

        if byte < 128 or byte >= 192 then
            break;
        end

        idx = idx - 1;
    end

    return text:sub(1, idx - 1);
end

local function push_line(text, r, g, b)
    lines[#lines + 1] = { text = text, r = r, g = g, b = b };

    while #lines > 200 do
        table.remove(lines, 1);
    end

    scroll = 0;
end

local frame = gui.create("Panel");
frame:set_visible(false);

local function layout()
    local w, h = surface.size();

    if not w or w < 1 then
        w = 1280;
    end

    if not h or h < 1 then
        h = 720;
    end

    frame:set_pos(0, 0);
    frame:set_size(w, math.floor(h * 0.45));
    frame:set_visible(open);
end

local function set_open(next_open)
    open = next_open and true or false;
    input.block_look(open);
    layout();
end

local function run_line()
    local text = input_text;
    input_text = "";
    matches = {};
    draft = "";

    if text ~= "" then
        history[#history + 1] = text;
    end

    history_at = #history + 1;
    push_line("] " .. text, 230, 230, 230);
    local results = console.submit(text);

    if not results then
        return;
    end

    local idx = 1;

    while idx <= #results do
        local row = results[idx];
        push_line((row.side or "") .. ": " .. (row.text or ""), 170, 210, 170);

        if row.detail and row.detail ~= "" then
            push_line(row.detail, 200, 200, 200);
        end

        if row.error and row.error ~= "" then
            push_line(row.error, 255, 140, 140);
        end

        idx = idx + 1;
    end
end

frame:set_paint(function(self, x, y, w, h)
    if open then
        input.block_look(true);
    end

    layout();
    surface.draw_rect(x, y, w, h, 10, 12, 16, 220);
    local line_h = 18;
    local input_y = y + h - line_h - 8;
    local top = input_y - line_h;

    if #matches > 0 then
        surface.draw_text("default", table.concat(matches, "  "), x + 8, top, 16, 190, 190, 140, 255);
        top = top - line_h;
    end

    local idx = #lines - scroll;

    while idx >= 1 and top >= y + 6 do
        local row = lines[idx];
        surface.draw_text("default", row.text, x + 8, top, 16, row.r, row.g, row.b, 255);
        top = top - line_h;
        idx = idx - 1;
    end

    surface.draw_text("default", "] " .. input_text .. "_", x + 8, input_y, 16, 245, 245, 245, 255);
end);

hook.add("GuiKeyPressed", "console", function(key, repeated)
    if key == "`" then
        if not repeated then
            set_open(not open);
        end

        return true;
    end

    if not open then
        return nil;
    end

    if key == "escape" then
        set_open(false);

        return true;
    end

    if key == "enter" or key == "kp_enter" then
        run_line();

        return true;
    end

    if key == "backspace" then
        input_text = pop_char(input_text);
        matches = {};

        return true;
    end

    if key == "tab" and not repeated then
        local text, list = console.complete(input_text);
        input_text = text or input_text;
        matches = list or {};

        return true;
    end

    if key == "uparrow" then
        if history_at == 0 then
            history_at = #history + 1;
        end

        if history_at == #history + 1 then
            draft = input_text;
        end

        if history_at > 1 then
            history_at = history_at - 1;
            input_text = history[history_at] or "";
            matches = {};
        end

        return true;
    end

    if key == "downarrow" then
        if history_at > 0 and history_at <= #history then
            history_at = history_at + 1;

            if history_at > #history then
                input_text = draft;
            else
                input_text = history[history_at] or "";
            end

            matches = {};
        end

        return true;
    end

    if key == "pgup" then
        scroll = math.min(#lines, scroll + 4);

        return true;
    end

    if key == "pgdn" then
        scroll = math.max(0, scroll - 4);

        return true;
    end

    return true;
end);

hook.add("GuiText", "console", function(text)
    if not open then
        return nil;
    end

    if text == "`" or text == "~" then
        return true;
    end

    input_text = input_text .. text;
    matches = {};

    return true;
end);
