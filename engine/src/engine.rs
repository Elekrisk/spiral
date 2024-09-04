use std::{borrow::BorrowMut, cell::{Ref, RefCell, RefMut}, collections::HashMap, fs::File, ops::Deref, path::Path, rc::Rc};

use log::error;
use mlua::UserData;
use ropey::Rope;

use crate::{
    buffer::{Buffer, BufferId},
    command::{builtin_commands, Command},
    keybind::{default_keybinds, Keybindings},
    view::{View, ViewId},
};

#[derive(Clone)]
pub struct Engine {
    pub state: Rc<RefCell<EngineState>>,
}

pub struct EngineState {
    pub lua: &'static mlua::Lua,
    pub buffers: HashMap<BufferId, Buffer>,
    pub views: HashMap<ViewId, View>,
    pub active_view: ViewId,

    pub keybinds: Keybindings,
    pub commands: HashMap<String, Command>,

    pub in_cli: bool,
    pub cli_contents: String,
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

    pub fn load_lua(&self, path: &str) {
        let lua = self.state.borrow().lua;
        lua.load(std::fs::read_to_string(path).unwrap())
            .set_name(path)
            .exec()
            .unwrap();
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

    pub fn key_event(&self, key: char) {
        let state = self.state();
        let Some(command) = state.keybinds.binds.get(&key) else { return };
        let mut words = command.split_ascii_whitespace();
        let Some(cmd) = words.next() else { return };
        let cmd = cmd.to_string();
        let args = words.map(String::from).collect();
        let Some(command) = state.commands.get(&cmd) else { error!("Unknown command {cmd}"); return };
        match command {
            Command::Builtin(cmd) => {
                let action = cmd.action.clone();
                drop(state);
                action.call(self.clone(), args)
            },
            Command::Custom(func) => {
                let func = func.clone();
                drop(state);
                func.call::<_, ()>(args).unwrap();
            },
        }
    }
}

impl EngineState {
    pub fn new() -> Self {
        let scratch_buffer = Buffer::create_from_contents("*scratch*".into(), Rope::new());

        let view = View::new(scratch_buffer.id);

        EngineState {
            lua: Box::leak(Box::new(mlua::Lua::new())),
            buffers: [(scratch_buffer.id, scratch_buffer)].into(),
            active_view: view.id,
            views: [(view.id, view)].into(),
            keybinds: default_keybinds(),
            commands: builtin_commands()
                .into_iter()
                .map(|c| (c.name.clone(), Command::Builtin(c)))
                .collect(),
            in_cli: false,
            cli_contents: String::new(),
        }
    }

    pub fn create_view(&mut self, buffer: BufferId) -> ViewId {
        let view = View::new(buffer);
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
        let buffer = Buffer::create_from_contents(path.to_string_lossy().to_string(), rope);
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
}
