-- Default config
-- Do not edit this file; it will be overwritten on package update

Editor.bind(":", "enter-command-mode")
Editor.bind("C-r", "reload-config")

Editor.bind("i", "enter-mode insert")

Editor.bind("h", "move-char-left")
Editor.bind("j", "move-char-down")
Editor.bind("k", "move-char-up")
Editor.bind("l", "move-char-right")
Editor.bind("H", "extend-char-left")
Editor.bind("J", "extend-char-down")
Editor.bind("K", "extend-char-up")
Editor.bind("L", "extend-char-right")
Editor.bind("g g", "goto-start")
Editor.bind("g e", "goto-end")
Editor.bind("g h", "goto-start-of-line")
Editor.bind("g l", "goto-end-of-line")
Editor.bind("g G", "extend-start")
Editor.bind("g E", "extend-end")
Editor.bind("g H", "extend-start-of-line")
Editor.bind("g L", "extend-end-of-line")
Editor.bind("u", "undo")
Editor.bind("U", "redo")

Editor.register_command("extend-selection-to-lines", "Extend current selection to entire lines", function()
    local view = Editor.get_active_view()
    if #(view:get_selections()) ~= 1 then
        return
    end

    local sel = view:get_selections()[1]

    sel.direction = "back"
    view:set_selections({sel})
    Editor.exec("extend-start-of-line")
    local sel = view:get_selections()[1]
    sel.direction = "forward"
    view:set_selections({sel})
    Editor.exec("extend-end-of-line")
end)
Editor.bind("x", "extend-selection-to-lines")

Editor.bind("%", "normal", "goto-start", "extend-end")

Editor.bind("d", "delete")
Editor.bind("c", "normal", "delete", "enter-mode insert")

Editor.bind("spc f", "open-file-tree")

Editor.register_command("open-file-tree", "Open file tree", function()
    local buffer = Editor.create_buffer()
    local view = Editor.create_view_for_buffer(buffer)
    Editor.set_active_view(view)
    -- pretend we got these through the filesystem
    local files = {
        "test.txt",
        "config.lua",
        "log.log",
        "Cargo.toml",
        "Cargo.lock",
    }
    for i, file in ipairs(files) do
        Editor.exec("insert \"" .. file .. "\\n\"")
    end
    Editor.exec("goto-start")
    Editor.exec("enter-mode file-tree")
    Editor.exec("extend-selection-to-lines")
end)

Editor.register_command("file-tree-open-current", "Open currently hovered file", function()
    local view = Editor.get_active_view()
    local sel = view:get_selections()[1]
    local path = sel:get_text()
    local path = path:gsub("%s+", "")
    Editor.exec("enter-mode normal")
    Editor.open_file(path)
end)

Editor.bind("j", "file-tree", "move-char-down", "extend-selection-to-lines")
Editor.bind("k", "file-tree", "move-char-up", "extend-selection-to-lines")
Editor.bind("enter", "file-tree", "file-tree-open-current")

Editor.bind("bspc", "insert", "move-char-left", "delete")
Editor.bind("enter", "insert", "insert \"\\n\"")
Editor.bind("tab", "insert", "insert \"\\t\"")
