use std::{
    borrow::BorrowMut,
    cell::{Ref, RefCell, RefMut},
    collections::HashMap,
    fs::File,
    ops::Deref,
    path::{Path, PathBuf},
    rc::Rc,
    str::FromStr,
};

use log::{error, trace};
use mlua::UserData;
use ratatui::{
    crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    layout::Constraint,
    style::{Modifier, Style},
    widgets::Widget,
    Frame,
};
use ropey::Rope;

use crate::{
    buffer::{Buffer, BufferBacking, BufferId},
    command::{builtin_commands, Command, CommandArgParser},
    keybind::{Binding, Key, Keybindings},
    kill_ring::KillRing,
    mode::Mode,
    view::{View, ViewId, ViewWidget},
};

#[derive(Clone)]
pub struct Engine {
    pub state: Rc<RefCell<EngineState>>,
}

pub struct EngineState {
    pub should_quit: bool,
    pub lua: &'static mlua::Lua,
    pub buffers: HashMap<BufferId, Buffer>,
    pub views: HashMap<ViewId, View>,
    pub active_view: ViewId,

    pub keybinds: Keybindings,
    pub commands: HashMap<String, Command>,

    pub key_queue: Vec<Key>,

    pub current_mode: Mode,

    pub cli: CommandLine,
    pub error_log: Vec<String>,

    pub size: Size,

    pub kill_ring: KillRing,
}

#[derive(Clone, Copy)]
pub struct Size {
    pub width: usize,
    pub height: usize,
}

impl Engine {
    pub fn new() -> anyhow::Result<Self> {
        let s = Self {
            state: Rc::new(RefCell::new(EngineState::new())),
        };
        crate::lua::init_lua(s.clone())?;
        Ok(s)
    }

    pub fn state(&self) -> Ref<EngineState> {
        self.state.borrow()
    }

    pub fn state_mut(&self) -> RefMut<EngineState> {
        self.state.deref().borrow_mut()
    }

    pub fn reload_config(&self) -> anyhow::Result<()> {
        let mut paths = vec![];
        paths.push(PathBuf::from("/etc/spiral/config.lua"));
        // paths.push(PathBuf::from("config.lua"));

        let mut path = dirs::config_dir()
            .map(|mut p| {
                p.push("spiral");
                p
            })
            .unwrap_or(PathBuf::from("."));
        path.push("config.lua");
        let user_config_path = path.display().to_string();
        paths.push(path);

        paths.push("config.lua".into());

        paths.retain(|p| p.exists());

        if paths.is_empty() {
            anyhow::bail!("No lua config found; create one at {}", user_config_path);
        }

        self.state_mut().commands = builtin_commands().map(|c| (c.name.clone(), c)).collect();
        self.state_mut().keybinds.binds.clear();

        for path in paths {
            self.load_lua(&path)?;
        }

        Ok(())
    }

    pub fn load_lua(&self, path: impl AsRef<Path>) -> anyhow::Result<()> {
        let path = path.as_ref();
        let lua = self.state.borrow().lua;
        lua.load(std::fs::read_to_string(path)?)
            .set_name(path.to_string_lossy())
            .exec()?;
        Ok(())
    }

    pub fn create_view(&self, buffer: BufferId) -> ViewId {
        self.state_mut().create_view(buffer)
    }

    pub fn buffer(&self, id: BufferId) -> Option<Ref<Buffer>> {
        Ref::filter_map(self.state(), |s| s.buffer(id)).ok()
    }

    pub fn create_buffer(&self) -> BufferId {
        self.state_mut().create_buffer()
    }

    pub fn open(&self, path: impl AsRef<Path>) -> ViewId {
        self.state_mut().open(path)
    }

    pub fn get_open_buffers(&self) -> Vec<BufferId> {
        self.state().buffers.keys().copied().collect()
    }

    pub fn active_view(&self) -> ViewId {
        self.state().active_view
    }

    pub fn view(&self, id: ViewId) -> Option<Ref<View>> {
        Ref::filter_map(self.state(), |s| s.view(id)).ok()
    }

    pub fn event(&self, event: Event) -> anyhow::Result<bool> {
        match event {
            Event::FocusGained => {}
            Event::FocusLost => {}
            Event::Key(key) if key.kind != KeyEventKind::Release  => match key.code {
                KeyCode::Char('q') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(true)
                }
                _ => self.key_event(key),
            },
            Event::Mouse(_) => {}
            Event::Paste(_) => {}
            Event::Resize(width, height) => {
                self.state_mut().resize(Size {
                    width: width as usize,
                    height: height as usize,
                });
            }
            _ => {}
        }

        Ok(self.state().should_quit)
    }

    pub fn key_event(&self, key_ev: KeyEvent) {
        let key = Key {
            code: key_ev.code,
            modifiers: key_ev.modifiers,
        };
        let mut state = self.state_mut();

        if state.cli.focus {
            match state.cli.key_event(key_ev) {
                CommandLineEvent::None => {}
                CommandLineEvent::Cancel => {
                    state.cli.focus = false;
                }
                CommandLineEvent::Exec(cmd) => {
                    drop(state);
                    if let Err(e) = self.execute_command(&cmd) {
                        error!("{e}");
                        self.state_mut().error_log.push(format!("{e}"));
                    }
                }
            }
            return;
        }

        if key.code == KeyCode::Esc && key.modifiers.is_empty() {
            if !state.key_queue.is_empty() {
                state.key_queue.clear();
            } else if !matches!(state.current_mode, Mode::Normal) {
                state.current_mode = Mode::Normal;
            }
            return;
        }

        let mut keys = state.key_queue.clone();
        keys.push(key);
        let Some(binding) = state.keybinds.get(&state.current_mode, &keys) else {
            state.key_queue.clear();

            if matches!(state.current_mode, Mode::Insert)
                && let KeyCode::Char(c) = key.code
            {
                let (mut view, mut buffer) = RefMut::map_split(state, |s| {
                    let view = s.views.get_mut(&s.active_view).unwrap();
                    let buffer_id = view.buffer;
                    let buffer = s.buffers.get_mut(&buffer_id).unwrap();
                    (view, buffer)
                });
                let mut selections = view
                    .selections
                    .iter()
                    .copied()
                    .enumerate()
                    .collect::<Vec<_>>();
                selections.sort_by_key(|s| s.1.start);

                for i in 0..selections.len() {
                    let s = selections[i].1;
                    buffer.contents.insert_char(s.start, c);
                    for (_, sel) in &mut selections[i..] {
                        sel.start += 1;
                        sel.end += 1;
                    }
                    view.selections[selections[i].0] = selections[i].1;
                }
            }
            return;
        };

        match binding {
            Binding::Group(_) => {
                state.key_queue.push(key);
            }
            Binding::Commands(cmd) => {
                let cmd = cmd.clone();
                state.key_queue.clear();
                drop(state);
                for cmd in cmd {
                    if let Err(e) = self.execute_command(&cmd) {
                        error!("{e}");
                        self.state_mut().error_log.push(format!("{e}"));
                        break;
                    }
                }
            }
        }
    }

    pub fn execute_command(&self, command: &str) -> anyhow::Result<()> {
        let (cmd, args) = command
            .split_once(|c: char| c.is_whitespace())
            .unwrap_or((command, ""));
        let state = self.state();
        let mut parser = CommandArgParser::new(args);
        let args = parser.args()?;

        let Some(command) = state.commands.get(cmd) else {
            anyhow::bail!("Unknown command {cmd}");
        };
        let action = command.action.clone();
        drop(state);
        action(self.clone(), args)
    }

    pub fn draw(&self, frame: &mut Frame) {
        self.state().draw(frame);
    }
}

impl EngineState {
    pub fn new() -> Self {
        let scratch_buffer = Buffer::create_from_contents("*scratch*".into(), Rope::new());

        let (width, height) = ratatui::crossterm::terminal::size().unwrap();
        let size = Size {
            width: width as usize,
            height: height as usize,
        };
        let view = View::new(scratch_buffer.id, size);

        EngineState {
            should_quit: false,
            lua: Box::leak(Box::new(mlua::Lua::new())),
            buffers: [(scratch_buffer.id, scratch_buffer)].into(),
            active_view: view.id,
            views: [(view.id, view)].into(),
            keybinds: Keybindings {
                binds: HashMap::new(),
            },
            key_queue: vec![],
            commands: builtin_commands().map(|c| (c.name.clone(), c)).collect(),
            current_mode: Mode::Normal,
            cli: CommandLine::new(),
            error_log: vec![],
            size,
            kill_ring: KillRing::new(),
        }
    }

    pub fn create_view(&mut self, buffer: BufferId) -> ViewId {
        let size = Size {
            width: self.size.width,
            height: self.size.height.saturating_sub(2),
        };
        let view = View::new(buffer, size);
        let view_id = view.id;
        self.views.insert(view_id, view);
        view_id
    }

    pub fn buffer(&self, id: BufferId) -> Option<&Buffer> {
        self.buffers.get(&id)
    }

    pub fn create_buffer(&mut self) -> BufferId {
        let buffer = Buffer::create_from_contents("*scratch*".into(), Rope::new());
        let buffer_id = buffer.id;
        self.buffers.insert(buffer_id, buffer);
        buffer_id
    }

    pub fn open(&mut self, path: impl AsRef<Path>) -> ViewId {
        let path = path.as_ref();
        let rope = ropey::Rope::from_reader(File::open(path).unwrap()).unwrap();
        let mut buffer = Buffer::create_from_contents(path.to_string_lossy().to_string(), rope);
        buffer.set_backing(BufferBacking::File(path.to_path_buf()));
        let buffer_id = buffer.id;
        self.buffers.insert(buffer_id, buffer);

        let view = self.create_view(buffer_id);
        self.active_view = view;
        view
    }

    pub fn active_view(&self) -> ViewId {
        self.active_view
    }

    pub fn view(&self, id: ViewId) -> Option<&View> {
        self.views.get(&id)
    }

    pub fn resize(&mut self, size: Size) {
        let view_size = Size {
            width: size.width,
            height: size.height.saturating_sub(2),
        };
        for view in self.views.values_mut() {
            view.resize(view_size);
            view.make_selection_visisble(self.buffers.get(&view.buffer).unwrap());
        }
    }

    pub fn draw(&self, frame: &mut Frame) {
        let view = self.view(self.active_view).unwrap();
        let buffer = self.buffer(view.buffer).unwrap();
        let widget = ViewWidget {
            view,
            buffer,
            mode: &self.current_mode,
        };
        let status_line = StatusLineWidget {
            mode: &self.current_mode,
        };
        let cmd_line = CommandLineWidget {
            command_line: &self.cli,
            error_log: &self.error_log,
        };

        let layout = ratatui::layout::Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(frame.area());

        frame.render_widget(widget, layout[0]);
        frame.render_widget(status_line, layout[1]);
        frame.render_widget(cmd_line, layout[2]);
    }
}

pub struct CommandLine {
    pub focus: bool,
    pub contents: String,
    pub cursor: usize,
}

impl CommandLine {
    pub fn new() -> Self {
        Self {
            focus: false,
            contents: String::new(),
            cursor: 0,
        }
    }

    pub fn key_event(&mut self, key: KeyEvent) -> CommandLineEvent {
        match key.code {
            KeyCode::Backspace if self.cursor > 0 => {
                self.contents.remove(self.cursor - 1);
                self.cursor -= 1;
            }
            KeyCode::Enter => {
                self.focus = false;
                self.cursor = 0;
                return if self.contents.trim().is_empty() {
                    self.contents.clear();
                    CommandLineEvent::Cancel
                } else {
                    CommandLineEvent::Exec(std::mem::take(&mut self.contents))
                };
            }
            KeyCode::Left if self.cursor > 0 => {
                self.cursor -= 1;
            }
            KeyCode::Right if self.cursor < self.contents.len() => {
                self.cursor += 1;
            }
            KeyCode::Up => {}
            KeyCode::Down => {}
            KeyCode::Home if self.cursor > 0 => {
                self.cursor = 0;
            }
            KeyCode::End if self.cursor < self.contents.len() => {
                self.cursor = self.contents.len();
            }
            KeyCode::Tab => {}
            KeyCode::BackTab => {}
            KeyCode::Delete if self.cursor < self.contents.len() => {
                self.contents.remove(self.cursor);
            }
            KeyCode::Char(c) => {
                self.contents.insert(self.cursor, c);
                self.cursor += 1;
            }
            KeyCode::Esc => {
                self.focus = false;
                self.contents.clear();
                self.cursor = 0;
                return CommandLineEvent::Cancel;
            }
            _ => {}
        }
        CommandLineEvent::None
    }
}

pub enum CommandLineEvent {
    None,
    Cancel,
    Exec(String),
}

pub struct CommandLineWidget<'a> {
    pub command_line: &'a CommandLine,
    pub error_log: &'a [String],
}

impl<'a> Widget for CommandLineWidget<'a> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        if self.command_line.focus {
            buf[(area.x, area.y)].set_char(':');
            buf.set_string(
                area.x + 1,
                area.y,
                &self.command_line.contents,
                Style::new(),
            );
            buf[(area.x + 1 + self.command_line.cursor as u16, area.y)]
                .modifier
                .insert(Modifier::REVERSED);
        } else if let Some(err) = self.error_log.last() {
            buf.set_string(
                area.x,
                area.y,
                err,
                Style::new().fg(ratatui::style::Color::Red),
            );
        }
    }
}

pub struct StatusLineWidget<'a> {
    pub mode: &'a Mode,
}

impl<'a> Widget for StatusLineWidget<'a> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        buf.set_style(area, Style::new().bg(ratatui::style::Color::DarkGray));
        buf.set_stringn(area.x, area.y, self.mode.to_string(), 8, Style::new());
    }
}
