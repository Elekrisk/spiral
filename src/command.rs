use std::{
    cell::{RefCell, RefMut},
    collections::HashMap,
    fmt::Display,
    iter::Peekable,
    rc::Rc,
    str::FromStr,
    usize,
};

use log::error;
use mlua::IntoLua;
use ratatui::buffer;
use ropey::Rope;
use tree_sitter::{InputEdit, Node, Point};

use crate::{
    buffer::{Action, Buffer, BufferBacking, BufferId, HistoryAction},
    engine::{Engine, EngineState},
    keybind::{Binding, Key},
    kill_ring::KillRingEntry,
    selection::Selection,
    view::{View, ViewId},
};

pub struct Command {
    pub name: String,
    pub desc: String,
    pub action: Rc<dyn Fn(Engine, Vec<CommandArg>) -> anyhow::Result<()>>,
}

impl Command {
    pub fn new<M>(
        name: impl Into<String>,
        desc: impl Into<String>,
        action: impl CommandAction<M> + 'static,
    ) -> Self {
        Self {
            name: name.into(),
            desc: desc.into(),
            action: Rc::new(move |engine, args| action.apply(engine, args)),
        }
    }

    pub fn new_lua(
        name: impl Into<String>,
        desc: impl Into<String>,
        action: mlua::Function<'static>,
    ) -> Self {
        Self {
            name: name.into(),
            desc: desc.into(),
            action: Rc::new(move |engine, args| {
                action.call::<_, ()>(args)?;
                Ok(())
            }),
        }
    }
}

fn views_buffers<'a>(
    state: RefMut<EngineState>,
) -> (
    RefMut<HashMap<ViewId, View>>,
    RefMut<HashMap<BufferId, Buffer>>,
) {
    RefMut::map_split(state, |s| (&mut s.views, &mut s.buffers))
}

fn view_buffer<'a>(state: RefMut<EngineState>) -> (RefMut<View>, RefMut<Buffer>) {
    let view_id = state.active_view;
    let (mut views, buffers) = views_buffers(state);
    let view = RefMut::map(views, |v| v.get_mut(&view_id).unwrap());
    let buffer = RefMut::map(buffers, |b| b.get_mut(&view.buffer).unwrap());
    (view, buffer)
}

fn for_selection_mut(engine: Engine, mut f: impl FnMut(&mut Selection, &mut Buffer)) {
    let state = engine.state_mut();
    let (mut view, mut buffer) = view_buffer(state);
    for selection in &mut view.selections {
        f(selection, &mut buffer);
    }
    view.make_selection_visisble(&buffer);
}

fn get_head_pos(selection: &Selection, buffer: &Buffer) -> (usize, usize) {
    let head = selection.head();
    let line = buffer.contents.char_to_line(head);
    let col = head - buffer.contents.line_to_char(line);
    (line, col)
}

fn set_head_pos(selection: &mut Selection, buffer: &Buffer, line: usize, col: usize) {
    let line = line.min(buffer.contents.len_lines());
    let max_col = if line == buffer.contents.len_lines() - 1 {
        buffer.contents.line(line).len_chars()
    } else {
        buffer.contents.line(line).len_chars().saturating_sub(1)
    };
    let col = col.min(max_col);
    *selection.head_mut() = buffer.contents.line_to_char(line) + col;
    selection.make_valid(&buffer.contents);
}

fn collapse_cursor(selection: &mut Selection) {
    let (head, anchor) = selection.head_anchor_mut();
    *anchor = *head;
}

// -- COMAMNDS --

fn move_char_right(engine: Engine) {
    for_selection_mut(engine, |sel, buf| {
        let (head, anchor) = sel.head_anchor_mut();
        *head += 1;
        sel.make_valid(&buf.contents);
    });
}

fn move_char_left(engine: Engine) {
    for_selection_mut(engine, |sel, buf| {
        let (head, anchor) = sel.head_anchor_mut();
        *head = head.saturating_sub(1);
        sel.make_valid(&buf.contents);
    });
}

fn move_char_up(engine: Engine) {
    for_selection_mut(engine, |sel, buf| {
        let (line, col) = get_head_pos(sel, buf);
        if line == 0 {
            *sel.head_mut() = 0;
            sel.make_valid(&buf.contents);
        } else {
            set_head_pos(sel, buf, line.saturating_sub(1), col);
        }
    });
}

fn move_char_down(engine: Engine) {
    for_selection_mut(engine, |sel, buf| {
        let (line, col) = get_head_pos(sel, buf);
        if line + 1 >= buf.contents.len_lines() {
            *sel.head_mut() = usize::MAX;
            sel.make_valid(&buf.contents);
        } else {
            set_head_pos(sel, buf, line + 1, col);
        }
    });
}

fn delete(engine: Engine) {
    let mut state = engine.state_mut();
    let state = &mut *state;
    let view = state.views.get_mut(&state.active_view).unwrap();
    let buffer = state.buffers.get_mut(&view.buffer).unwrap();

    let mut texts = vec![];
    let mut actions = vec![];

    for i in 0..view.selections.len() {
        let s = view.selections[i];

        let text = buffer.contents.slice(s.start..=s.end).to_string();
        texts.push(text.clone());

        buffer.remove(view, s.start, s.end - s.start + 1);
        actions.push(Action::TextDeletion {
            deleted_text: text,
            start: s.start,
            len: s.end - s.start + 1,
        });
    }

    buffer.history.register_edit(HistoryAction { actions });
    buffer.recalc_tree();

    state.kill_ring.add_entry(KillRingEntry::new(texts));

    view.merge_overlapping_selections();
    view.make_selection_visisble(buffer);
}

fn backspace(engine: Engine) {
    let state = engine.state_mut();
    let (mut view, mut buffer) = view_buffer(state);

    let mut actions = vec![];

    for i in 0..view.selections.len() {
        let s = view.selections[i];
        if s.start == 0 {
            continue;
        }

        let text = buffer.contents.slice(s.start - 1..s.start).to_string();
        buffer.remove(&mut view, s.start - 1, 1);

        actions.push(Action::TextDeletion {
            deleted_text: text,
            start: s.start - 1,
            len: 1,
        });
    }

    buffer.history.register_edit(HistoryAction { actions });
    buffer.recalc_tree();

    view.merge_overlapping_selections();
    view.make_selection_visisble(&buffer);
}

fn insert(engine: Engine, text: String) {
    let state = engine.state_mut();
    let (mut view, mut buffer) = view_buffer(state);

    let mut actions = vec![];

    for i in 0..view.selections.len() {
        let s = view.selections[i];
        buffer.insert(&mut view, &text, s.start);
        let action = Action::TextInsertion {
            text: text.clone(),
            start: s.start,
        };
        actions.push(action);
    }

    buffer.history.register_edit(HistoryAction { actions });
    buffer.recalc_tree();

    view.make_selection_visisble(&buffer);
}

fn goto_end_of_line(engine: Engine, collapse: bool) {
    for_selection_mut(engine, |sel, buf| {
        let (line, col) = get_head_pos(sel, buf);
        set_head_pos(sel, buf, line, usize::MAX);
        if collapse {
            collapse_cursor(sel);
        }
        sel.make_valid(&buf.contents);
    });
}

fn goto_start_of_line(engine: Engine, collapse: bool) {
    for_selection_mut(engine, |sel, buf| {
        let (line, col) = get_head_pos(sel, buf);
        set_head_pos(sel, buf, line, 0);
        if collapse {
            collapse_cursor(sel);
        }
        sel.make_valid(&buf.contents);
    });
}

fn goto_start(engine: Engine, collapse: bool) {
    for_selection_mut(engine, |sel, buf| {
        let (head, anchor) = sel.head_anchor_mut();
        *head = 0;
        if collapse {
            *anchor = 0;
        }
        sel.make_valid(&buf.contents);
    });
}

fn goto_end(engine: Engine, collapse: bool) {
    for_selection_mut(engine, |sel, buf| {
        let len = buf.contents.len_chars();
        let (head, anchor) = sel.head_anchor_mut();
        *head = len;
        if collapse {
            *anchor = len;
        }
        sel.make_valid(&buf.contents);
    });
}

fn undo(engine: Engine) {
    let mut state = engine.state_mut();
    let state = &mut *state;
    let view = state.views.get_mut(&state.active_view).unwrap();
    let buffer = state.buffers.get_mut(&view.buffer).unwrap();

    buffer.undo(view);
}

fn redo(engine: Engine) {
    let mut state = engine.state_mut();
    let state = &mut *state;
    let view = state.views.get_mut(&state.active_view).unwrap();
    let buffer = state.buffers.get_mut(&view.buffer).unwrap();

    buffer.redo(view);
}

fn show_kill_ring(engine: Engine) {
    let mut state = engine.state_mut();
    let buffer_id = state.create_buffer();
    let view_id = state.create_view(buffer_id);
    state.active_view = view_id;

    let mut contents = String::new();
    for entry in &state.kill_ring.entries {
        use std::fmt::Write;
        for text in &entry.text {
            write!(&mut contents, "{text:?}, ").unwrap();
        }
        writeln!(&mut contents).unwrap();
    }
    let buffer = state.buffers.get_mut(&buffer_id).unwrap();

    buffer.contents = contents.into();
}

fn copy_kill_ring(engine: Engine) {
    let mut state = engine.state_mut();
    let state = &mut *state;

    let active_view = state.active_view;
    let view = state.views.get_mut(&active_view).unwrap();
    let buffer = state.buffers.get_mut(&view.buffer).unwrap();

    state
        .kill_ring
        .add_entry(KillRingEntry::new(view.selections.iter().map(
            |selection| {
                buffer
                    .contents
                    .slice(selection.start..(selection.end + 1).min(buffer.contents.len_chars()))
                    .to_string()
            },
        )));
}

fn paste_kill_ring(engine: Engine, before: bool) {
    let mut state = engine.state_mut();
    let state = &mut *state;

    if state.kill_ring.entries.is_empty() {
        return;
    }

    let view = state.views.get_mut(&state.active_view).unwrap();
    let buffer = state.buffers.get_mut(&view.buffer).unwrap();

    let mut actions = vec![];

    let texts = state
        .kill_ring
        .get()
        .unwrap()
        .get_for_cursor_count(view.selections.len());

    for i in 0..view.selections.len() {
        let s = view.selections[i];
        let start = (if before { s.start } else { s.end + 1 }).min(buffer.contents.len_chars());
        buffer.insert(view, texts[i], start);
        let action = Action::TextInsertion {
            text: texts[0].to_string(),
            start,
        };
        actions.push(action);
    }

    buffer.history.register_edit(HistoryAction { actions });
    buffer.recalc_tree();

    view.make_selection_visisble(buffer);
}

fn close_buffer(engine: Engine) {
    let mut state = engine.state_mut();
    let state = &mut *state;
    let view = state.active_view;
    let view = state.views.remove(&view).unwrap();
    let buffer = state.buffers.get_mut(&view.buffer).unwrap();
    buffer.view_count -= 1;
    if buffer.view_count == 0 {
        state.buffers.remove(&view.buffer).unwrap();
    }

    state.active_view = match state.views.keys().next() {
        Some(id) => *id,
        None => {
            let buffer = state.create_buffer();
            state.create_view(buffer)
        }
    }
}

fn list_buffers(engine: Engine) {
    let mut state = engine.state_mut();
    let state = &mut *state;
    let buffer_id = state.create_buffer();
    let view_id = state.create_view(buffer_id);
    state.active_view = view_id;

    let mut contents = String::new();
    for (id, buffer) in &state.buffers {
        use std::fmt::Write;
        writeln!(&mut contents, "{}: {}", id.0, buffer.name).unwrap();
    }
    let buffer = state.buffers.get_mut(&buffer_id).unwrap();

    buffer.contents = contents.into();
}

fn tree_sitter_out(engine: Engine) {
    let mut state = engine.state_mut();
    let state = &mut *state;
    let view = state.views.get_mut(&state.active_view).unwrap();
    let buffer = state.buffers.get_mut(&view.buffer).unwrap();

    for sel in &mut view.selections {
        let start = buffer.contents.char_to_byte(sel.start);
        let end = buffer.contents.char_to_byte(sel.end + 1);
        if let Some(node) = buffer
            .tree
            .root_node()
            .descendant_for_byte_range(start, end)
        {
            let mut range = node.byte_range();
            if range.start == start
                && range.end == end
                && let Some(node) = node.parent()
            {
                range = node.byte_range();
            }

            sel.start = buffer.contents.byte_to_char(range.start);
            sel.end = buffer.contents.byte_to_char(range.end) - 1;
        }
    }

    view.merge_overlapping_selections();
    view.make_selection_visisble(buffer);
}

fn tree_sitter_in(engine: Engine) {
    let mut state = engine.state_mut();
    let state = &mut *state;
    let view = state.views.get_mut(&state.active_view).unwrap();
    let buffer = state.buffers.get_mut(&view.buffer).unwrap();

    for sel in &mut view.selections {
        let start = buffer.contents.char_to_byte(sel.start);
        let end = buffer.contents.char_to_byte(sel.end + 1);
        if let Some(node) = buffer
            .tree
            .root_node()
            .descendant_for_byte_range(start, end)
        {
            let mut range = node.byte_range();
            if let Some(node) = node.child(0) {
                range = node.byte_range();
            }

            sel.start = buffer.contents.byte_to_char(range.start);
            sel.end = buffer.contents.byte_to_char(range.end) - 1;
        }
    }

    view.merge_overlapping_selections();
    view.make_selection_visisble(buffer);
}

fn tree_sitter_next(engine: Engine) {
    let mut state = engine.state_mut();
    let state = &mut *state;
    let view = state.views.get_mut(&state.active_view).unwrap();
    let buffer = state.buffers.get_mut(&view.buffer).unwrap();

    for sel in &mut view.selections {
        let start = buffer.contents.char_to_byte(sel.start);
        let end = buffer.contents.char_to_byte(sel.end + 1);
        if let Some(node) = buffer
            .tree
            .root_node()
            .descendant_for_byte_range(start, end)
        {
            let mut range = node.byte_range();
            if let Some(node) = node.next_sibling() {
                range = node.byte_range();
            }

            sel.start = buffer.contents.byte_to_char(range.start);
            sel.end = buffer.contents.byte_to_char(range.end) - 1;
        }
    }

    view.merge_overlapping_selections();
    view.make_selection_visisble(buffer);
}

fn tree_sitter_prev(engine: Engine) {
    let mut state = engine.state_mut();
    let state = &mut *state;
    let view = state.views.get_mut(&state.active_view).unwrap();
    let buffer = state.buffers.get_mut(&view.buffer).unwrap();

    for sel in &mut view.selections {
        let start = buffer.contents.char_to_byte(sel.start);
        let end = buffer.contents.char_to_byte(sel.end + 1);
        if let Some(node) = buffer
            .tree
            .root_node()
            .descendant_for_byte_range(start, end)
        {
            let mut range = node.byte_range();
            if let Some(node) = node.prev_sibling() {
                range = node.byte_range();
            }

            sel.start = buffer.contents.byte_to_char(range.start);
            sel.end = buffer.contents.byte_to_char(range.end) - 1;
        }
    }

    view.merge_overlapping_selections();
    view.make_selection_visisble(buffer);
}

pub fn builtin_commands() -> impl Iterator<Item = Command> {
    [
        Command::new(
            "enter-command-mode",
            "Enter command mode",
            |engine: Engine| {
                engine.state_mut().cli.focus = true;
            },
        ),
        Command::new(
            "move-char-right",
            "Move one char right",
            |engine: Engine| {
                move_char_right(engine.clone());
                for_selection_mut(engine, |sel, _| collapse_cursor(sel));
            },
        ),
        Command::new("move-char-left", "Move one char left", |engine: Engine| {
            move_char_left(engine.clone());
            for_selection_mut(engine, |sel, _| collapse_cursor(sel));
        }),
        Command::new("move-char-down", "Move one char down", |engine: Engine| {
            move_char_down(engine.clone());
            for_selection_mut(engine, |sel, _| collapse_cursor(sel));
        }),
        Command::new("move-char-up", "Move one char up", |engine: Engine| {
            move_char_up(engine.clone());
            for_selection_mut(engine, |sel, _| collapse_cursor(sel));
        }),
        Command::new(
            "extend-char-right",
            "Extend selection one char right",
            |engine: Engine| {
                move_char_right(engine);
            },
        ),
        Command::new(
            "extend-char-left",
            "Extend selection one char left",
            |engine: Engine| {
                move_char_left(engine);
            },
        ),
        Command::new(
            "extend-char-down",
            "Extend selection one char down",
            |engine: Engine| {
                move_char_down(engine);
            },
        ),
        Command::new(
            "extend-char-up",
            "Extend selection one char up",
            |engine: Engine| {
                move_char_up(engine);
            },
        ),
        Command::new("delete", "Delete selected text", |engine: Engine| {
            delete(engine);
        }),
        Command::new(
            "backspace",
            "Delete character before selection",
            |engine: Engine| {
                backspace(engine);
            },
        ),
        Command::new(
            "insert",
            "Insert given text before each selection",
            |engine: Engine, text: String| {
                insert(engine, text);
            },
        ),
        Command::new(
            "goto-start-of-line",
            "Goto start of line",
            |engine: Engine| {
                goto_start_of_line(engine, true);
            },
        ),
        Command::new("goto-end-of-line", "Goto end of line", |engine: Engine| {
            goto_end_of_line(engine, true);
        }),
        Command::new("goto-start", "Goto start of file", |engine: Engine| {
            goto_start(engine, true);
        }),
        Command::new("goto-end", "Goto end of file", |engine: Engine| {
            goto_end(engine, true);
        }),
        Command::new(
            "extend-start-of-line",
            "Extend selection to start of line",
            |engine: Engine| {
                goto_start_of_line(engine, false);
            },
        ),
        Command::new(
            "extend-end-of-line",
            "Extend selection to end of line",
            |engine: Engine| {
                goto_end_of_line(engine, false);
            },
        ),
        Command::new(
            "extend-start",
            "Extend selection to start of file",
            |engine: Engine| {
                goto_start(engine, false);
            },
        ),
        Command::new(
            "extend-end",
            "Extend selection to end of file",
            |engine: Engine| {
                goto_end(engine, false);
            },
        ),
        Command::new("undo", "Undo", |engine: Engine| {
            undo(engine);
        }),
        Command::new("redo", "Redo", |engine: Engine| {
            redo(engine);
        }),
        Command::new(
            "write",
            "Write buffer to disk or to given path",
            |engine: Engine, args: Vec<CommandArg>| {
                let path = args.into_iter().next();
                if let Some(path) = path {
                    let path: String = path.into();
                    let (_, mut buffer) = view_buffer(engine.state_mut());
                    buffer.backing = BufferBacking::File(path.try_into().unwrap());
                }

                let state = engine.state();
                let view = state.active_view;
                let view = state.view(view).unwrap();
                let buffer = state.buffer(view.buffer).unwrap();
                buffer.backing.save(&buffer)
            },
        ),
        Command::new("quit", "Quit Spiral", |engine: Engine| {
            engine.state_mut().should_quit = true;
        }),
        Command::new(
            "enter-mode",
            "Enter given mode",
            |engine: Engine, mode: String| {
                let mode = mode.parse()?;
                engine.state_mut().current_mode = mode;

                Ok(())
            },
        ),
        Command::new("reload-config", "Reload config", |engine: Engine| {
            if let Err(e) = engine.reload_config() {
                error!("{e}");
                engine.state_mut().error_log.push(e.to_string());
            }
        }),
        Command::new("binds", "Show current keybinds", |engine: Engine| {
            let mut state = engine.state_mut();
            let buffer = state.create_buffer();
            let view = state.create_view(buffer);
            state.active_view = view;

            let mut contents = String::new();

            for (mode, binds) in &state.keybinds.binds {
                use std::fmt::Write;
                writeln!(&mut contents, "{mode} {{").unwrap();

                let mut seq = vec![];

                fn print_binding<'a>(
                    contents: &mut String,
                    seq: &mut Vec<&'a Key>,
                    binding: &'a Binding,
                ) {
                    match binding {
                        Binding::Group(map) => {
                            for (key, binding) in map {
                                seq.push(key);
                                print_binding(contents, seq, binding);
                                seq.pop();
                            }
                        }
                        Binding::Commands(cmds) => {
                            writeln!(
                                contents,
                                "    {} -- {}",
                                seq.iter()
                                    .map(|k| k.to_string())
                                    .intersperse(String::from(" "))
                                    .collect::<String>(),
                                cmds.iter()
                                    .cloned()
                                    .intersperse(String::from(", "))
                                    .collect::<String>(),
                            )
                            .unwrap();
                        }
                    }
                }

                for (key, bind) in binds {
                    seq.push(key);
                    print_binding(&mut contents, &mut seq, bind);
                    seq.pop();
                }

                writeln!(&mut contents, "}}").unwrap();
            }

            let buffer = state.buffers.get_mut(&buffer).unwrap();
            buffer.contents = contents.into();
        }),
        Command::new("commands", "Show commands", |engine: Engine| {
            let mut state = engine.state_mut();
            let buffer = state.create_buffer();
            let view = state.create_view(buffer);
            state.active_view = view;

            let mut contents = String::new();

            for cmd in state.commands.values() {
                use std::fmt::Write;
                writeln!(&mut contents, "{}: {}", cmd.name, cmd.desc).unwrap();
            }
            state.buffers.get_mut(&buffer).unwrap().contents = contents.into();
        }),
        Command::new("show-kill-ring", "Show kill ring", |engine| {
            show_kill_ring(engine);
        }),
        Command::new(
            "paste-kill-ring",
            "Paste last item from kill ring",
            |engine, before: bool| {
                paste_kill_ring(engine, before);
            },
        ),
        Command::new("copy-kill-ring", "Copy selection to kill ring", |engine| {
            copy_kill_ring(engine);
        }),
        Command::new(
            "close-buffer",
            "Closes the current buffer view",
            close_buffer,
        ),
        Command::new("list-buffers", "Lists the open buffers", list_buffers),
        Command::new("tree-sitter-out", "TODO: Add desciption", tree_sitter_out),
        Command::new("tree-sitter-in", "TODO: Add desciption", tree_sitter_in),
        Command::new("tree-sitter-next", "TODO: Add desciption", tree_sitter_next),
        Command::new("tree-sitter-prev", "TODO: Add desciption", tree_sitter_prev),
    ]
    .into_iter()
}

pub struct CommandArgParser<'a> {
    chars: Peekable<std::str::Chars<'a>>,
}

#[derive(Clone, Copy)]
enum State {
    None,
    String(bool),
    Word,
}

impl<'a> CommandArgParser<'a> {
    pub fn new(str: &'a str) -> Self {
        Self {
            chars: str.chars().peekable(),
        }
    }

    pub fn args(&mut self) -> anyhow::Result<Vec<CommandArg>> {
        std::iter::from_fn(|| self.arg().transpose()).try_collect()
    }

    pub fn arg(&mut self) -> anyhow::Result<Option<CommandArg>> {
        while self.chars.peek().is_some_and(|c| c.is_whitespace()) {
            self.chars.next().unwrap();
        }

        let mut buf = String::new();
        let mut state = State::None;

        let res = loop {
            let Some(c) = self.chars.next() else {
                break match state {
                    State::None => None,
                    State::String(_) => anyhow::bail!("Unclosed string"),
                    State::Word => {
                        if let Ok(i) = buf.parse() {
                            Some(CommandArg::Integer(i))
                        } else {
                            match buf.as_str() {
                                "true" => Some(CommandArg::Bool(true)),
                                "false" => Some(CommandArg::Bool(false)),
                                _ => Some(CommandArg::String(buf)),
                            }
                        }
                    }
                };
            };
            match (state, c) {
                (State::None, '"') => {
                    state = State::String(false);
                }
                (State::None, _) => {
                    state = State::Word;
                    buf.push(c);
                }
                (State::Word, _) if !c.is_whitespace() => {
                    buf.push(c);
                }
                (State::Word, _) => {
                    break if let Ok(i) = buf.parse() {
                        Some(CommandArg::Integer(i))
                    } else {
                        match buf.as_str() {
                            "true" => Some(CommandArg::Bool(true)),
                            "false" => Some(CommandArg::Bool(false)),
                            _ => Some(CommandArg::String(buf)),
                        }
                    }
                }
                (State::String(false), '"') => {
                    break Some(CommandArg::String(buf));
                }
                (State::String(false), '\\') => {
                    state = State::String(true);
                }
                (State::String(false), o) => {
                    buf.push(o);
                }
                (State::String(true), '"') => {
                    buf.push('"');
                    state = State::String(false);
                }
                (State::String(true), 'n') => {
                    buf.push('\n');
                    state = State::String(false);
                }
                (State::String(true), 't') => {
                    buf.push('\t');
                    state = State::String(false);
                }
                (State::String(true), 'r') => {
                    buf.push('\r');
                    state = State::String(false);
                }
                (State::String(true), '\\') => {
                    buf.push('\\');
                    state = State::String(false);
                }
                (State::String(true), o) => {
                    anyhow::bail!("Invalid escape sequence '\\{o}'")
                }
            }
        };

        Ok(res)
    }
}

pub enum CommandArg {
    String(String),
    Integer(i32),
    Bool(bool),
}

#[derive(Debug, Clone)]
pub struct CommandArgError {
    expected: String,
    found: String,
}

impl Display for CommandArgError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Expected {}, found {}", self.expected, self.found)
    }
}

impl std::error::Error for CommandArgError {}

impl From<CommandArg> for String {
    fn from(value: CommandArg) -> Self {
        match value {
            CommandArg::String(s) => s,
            CommandArg::Integer(i) => i.to_string(),
            CommandArg::Bool(b) => b.to_string(),
        }
    }
}

impl TryFrom<CommandArg> for i32 {
    type Error = CommandArgError;

    fn try_from(value: CommandArg) -> Result<Self, Self::Error> {
        match value {
            CommandArg::String(_) => Err(CommandArgError {
                expected: "Integer".into(),
                found: "String".into(),
            }),
            CommandArg::Integer(i) => Ok(i),
            CommandArg::Bool(_) => Err(CommandArgError {
                expected: "Integer".into(),
                found: "Bool".into(),
            }),
        }
    }
}

impl TryFrom<CommandArg> for bool {
    type Error = CommandArgError;

    fn try_from(value: CommandArg) -> Result<Self, Self::Error> {
        match value {
            CommandArg::String(_) => Err(CommandArgError {
                expected: "Bool".into(),
                found: "String".into(),
            }),
            CommandArg::Integer(_) => Err(CommandArgError {
                expected: "Bool".into(),
                found: "Integer".into(),
            }),
            CommandArg::Bool(b) => Ok(b),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandArgParseError;

impl Display for CommandArgParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Could not parse command argument")
    }
}

impl std::error::Error for CommandArgParseError {}

impl FromStr for CommandArg {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(bool) = s.parse() {
            Ok(CommandArg::Bool(bool))
        } else if let Ok(int) = s.parse() {
            Ok(CommandArg::Integer(int))
        } else {
            Ok(CommandArg::String(s.into()))
        }
    }
}

impl<'lua> IntoLua<'lua> for CommandArg {
    fn into_lua(self, lua: &'lua mlua::Lua) -> mlua::Result<mlua::Value<'lua>> {
        match self {
            CommandArg::String(s) => lua.create_string(s).map(mlua::Value::String),
            CommandArg::Integer(i) => Ok(mlua::Value::Integer(i)),
            CommandArg::Bool(b) => Ok(mlua::Value::Boolean(b)),
        }
    }
}

pub trait CommandAction<M> {
    fn apply(&self, engine: Engine, args: Vec<CommandArg>) -> anyhow::Result<()>;
}

impl<F: Fn(Engine, Vec<CommandArg>)> CommandAction<((i8,),)> for F {
    fn apply(&self, engine: Engine, args: Vec<CommandArg>) -> anyhow::Result<()> {
        self(engine, args);
        Ok(())
    }
}
impl<F: Fn(Engine, Vec<CommandArg>) -> anyhow::Result<()>> CommandAction<((),)> for F {
    fn apply(&self, engine: Engine, args: Vec<CommandArg>) -> anyhow::Result<()> {
        self(engine, args)
    }
}

macro_rules! _impl_for {
    ($($ty:ident),* $(,)?) => {
        impl <Func, $($ty),*> CommandAction<($($ty,)*)> for Func
        where
            Func: Fn(Engine $(, $ty)*),
            $($ty: TryFrom<CommandArg>, <$ty as TryFrom<CommandArg>>::Error: std::error::Error + Send + Sync + 'static,)*
        {
            fn apply(&self, engine: Engine, args: Vec<CommandArg>) -> anyhow::Result<()> {
                #[allow(unused_mut)]
                #[allow(unused_variables)]
                let mut iter = args.into_iter();
                self(
                    engine,
                    $(${ignore($ty)} iter.next().unwrap().try_into()?,)*
                );
                Ok(())
            }
        }

        impl <Func, $($ty),*> CommandAction<(i8, ($($ty,)*))> for Func
        where
            Func: Fn(Engine $(, $ty)*) -> anyhow::Result<()>,
            $($ty: TryFrom<CommandArg>, <$ty as TryFrom<CommandArg>>::Error: std::error::Error + Send + Sync + 'static,)*
        {
            fn apply(&self, engine: Engine, args: Vec<CommandArg>) -> anyhow::Result<()> {
                #[allow(unused_mut)]
                #[allow(unused_variables)]
                let mut iter = args.into_iter();
                self(
                    engine,
                    $(${ignore($ty)} iter.next().unwrap().try_into()?,)*
                )
            }
        }
    };
}

macro_rules! impl_for {
    ($first:ident $(, $ty:ident)*) => {
        _impl_for!($first $(, $ty)*);
        impl_for!($($ty),*);
    };
    () => {
        _impl_for!();
    }
}

impl_for!(A, B, C, D, E, F, G, H, I);
