
Editor.register_command("next-buffer", function()
    local views = Editor.get_views()
    local active_view = Editor.get_active_view()
    local next = false
    for i = 1,#views do
        local cur = views[i]
        if next == true then
            Editor.set_active_view(cur)
            next = false
            break
        end
        if cur.id == active_view.id then
            next = true
        end
    end

    if next then
        Editor.set_active_view(views[1])
    end
end)

Editor.register_command("move-char-right", function()
    local view = Editor.get_active_view()
    local selections = view:get_selections()
    for i,selection in ipairs(selections) do
        selection.start += 1
        selection["end"] += 1
    end
    view:set_selections(selections)
end)

Editor.register_command("move-char-left", function()
    local view = Editor.get_active_view()
    local selections = view:get_selections()
    for i,selection in ipairs(selections) do
        selection.start -= 1
        selection["end"] -= 1
    end
    view:set_selections(selections)
end)

Editor.register_command("extend-char-right", function()
    local view = Editor.get_active_view()
    local selections = view:get_selections()
    for i,selection in ipairs(selections) do
        -- selection.start += 1
        selection["end"] += 1
    end
    view:set_selections(selections)
end)

Editor.register_command("extend-char-left", function()
    local view = Editor.get_active_view()
    local selections = view:get_selections()
    for i,selection in ipairs(selections) do
        selection.start -= 1
        -- selection["end"] -= 1
    end
    view:set_selections(selections)
end)

Editor.register_command("make-selection-single-char", function()
    local view = Editor.get_active_view()
    local selections = view:get_selections()
    for i,selection in ipairs(selections) do
        selection.start = selection["end"]
    end
    view:set_selections(selections)
end)

Editor.register_command("delete", function()
    local view = Editor.get_active_view()
    local selections = view:get_selections()
    for i, selection in ipairs(selections) do
        selection:set_text("")
        selection["end"] = selection.start
    end
    view:set_selections(selections)
end)

Editor.bind_key("n", "next-buffer")
Editor.bind_key("h", "move-char-left")
Editor.bind_key("l", "move-char-right")
Editor.bind_key("H", "extend-char-left")
Editor.bind_key("L", "extend-char-right")
Editor.bind_key("ö", "make-selection-single-char")
Editor.bind_key("d", "delete")
