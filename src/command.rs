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

use crate::{
    buffer::{Buffer, BufferBacking, BufferId},
    engine::{Engine, EngineState},
    history::{Action, HistoryAction},
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
    let col = col.min(buffer.contents.line(line).len_chars().saturating_sub(1));
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
    let state = engine.state_mut();
    let (mut view, mut buffer) = view_buffer(state);

    let mut selections = view
        .selections
        .iter()
        .copied()
        .enumerate()
        .collect::<Vec<_>>();

    selections.sort_by_key(|(_, s)| s.start);

    let mut len = buffer.contents.len_chars();

    let mut actions = vec![];

    for i in 0..selections.len() {
        let s = selections[i].1;
        let end = (s.end + 1).min(len);
        let rem_len = end - s.start;
        let text = buffer.contents.slice(s.start..end);
        let text = text.to_string();
        let action = Action::TextDeletion {
            deleted_text: text,
            start: s.start,
            end,
        };
        actions.push(action);
        buffer.contents.remove(s.start..end);
        len -= rem_len;
        for sel in selections[i + 1..].iter_mut().map(|(_, s)| s) {
            sel.start = (sel.start - rem_len).max(s.start);
            sel.end = (sel.end - rem_len).max(s.start);
        }
        selections[i].1.end = selections[i].1.start;
        view.selections[selections[i].0] = selections[i].1;
    }

    buffer.history.register_edit(HistoryAction { actions });

    view.make_selection_visisble(&buffer);
}

fn insert(engine: Engine, text: String) {
    let state = engine.state_mut();
    let (mut view, mut buffer) = view_buffer(state);

    let mut selections = view
        .selections
        .iter()
        .copied()
        .enumerate()
        .collect::<Vec<_>>();

    selections.sort_by_key(|(_, s)| s.start);

    let mut actions = vec![];

    for i in 0..selections.len() {
        let s = selections[i].1;
        let text_len = text.len();
        buffer.contents.insert(s.start, &text);
        let action = Action::TextInsertion {
            text: text.clone(),
            start: s.start,
        };
        actions.push(action);
        for sel in selections[i..].iter_mut().map(|(_, s)| s) {
            sel.start += text_len;
            sel.end += text_len;
        }
        view.selections[selections[i].0] = selections[i].1;
    }

    buffer.history.register_edit(HistoryAction { actions });

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
    let buffer_id = {
        let (_, buffer) = view_buffer(engine.state_mut());
        let buffer_id = buffer.id;
        let (mut history, mut text) =
            RefMut::map_split(buffer, |b| (&mut b.history, &mut b.contents));
        history.undo(&mut text);
        buffer_id
    };

    let state = engine.state_mut();
    let (mut views, buffers) = views_buffers(state);
    let buffer = buffers.get(&buffer_id).unwrap();

    for view in views.values_mut().filter(|v| v.buffer == buffer_id) {
        for sel in &mut view.selections {
            sel.make_valid(&buffer.contents);
        }
        view.make_selection_visisble(&buffer);
    }
}

fn redo(engine: Engine) {
    let buffer_id = {
        let (_, buffer) = view_buffer(engine.state_mut());
        let buffer_id = buffer.id;
        let (mut history, mut text) =
            RefMut::map_split(buffer, |b| (&mut b.history, &mut b.contents));
        history.redo(&mut text);
        buffer_id
    };

    let state = engine.state_mut();
    let (mut views, buffers) = views_buffers(state);
    let buffer = buffers.get(&buffer_id).unwrap();

    for view in views.values_mut().filter(|v| v.buffer == buffer_id) {
        for sel in &mut view.selections {
            sel.make_valid(&buffer.contents);
        }
        view.make_selection_visisble(&buffer);
    }
}

fn write(engine: Engine) {}

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
        Command::new("write", "Write buffer to disk or to given path", |engine: Engine, args: Vec<CommandArg>| {
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
        }),
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
            let mut state = engine.state_mut();
            state.commands = builtin_commands().map(|c| (c.name.clone(), c)).collect();
            state.keybinds.binds.clear();
            drop(state);
            if let Err(e) = engine.load_lua("./config.lua") {
                error!("{e}");
            }
        }),
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
