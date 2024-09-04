use std::{cell::RefCell, collections::HashMap, fs::File, path::Path, rc::Rc};

use log::error;
use mlua::UserData;
use ropey::Rope;

use crate::{
    buffer::{Buffer, BufferId},
    command::{builtin_commands, Command},
    keybind::Keybindings,
    view::{View, ViewId},
};

pub struct Engine {
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
    pub fn new() -> anyhow::Result<Rc<RefCell<Self>>> {
        let scratch_buffer = Buffer::create_from_contents("*scratch*".into(), Rope::new());

        let view_id = ViewId::generate();
        let view = View::new(scratch_buffer.id);

        let s = Rc::new(RefCell::new(Self {
            lua: Box::leak(Box::new(mlua::Lua::new())),
            buffers: [(scratch_buffer.id, scratch_buffer)].into(),
            views: [(view_id, view)].into(),
            active_view: view_id,
            keybinds: Keybindings {
                binds: HashMap::new(),
            },
            commands: builtin_commands()
                .into_iter()
                .map(|c| (c.name.clone(), Command::Builtin(c)))
                .collect(),
                in_cli: false,
                cli_contents: String::new(),
        }));
        crate::lua::init_lua(s.clone())?;
        Ok(s)
    }

    pub fn load_lua(&mut self, lua: &str) {
        self.lua
            .load(std::fs::read_to_string(lua).unwrap())
            .set_name(lua)
            .exec()
            .unwrap();
    }

    pub fn create_view(&mut self, buffer: BufferId) -> ViewId {
        let view = View::new(buffer);
        let view_id = view.id;
        self.views.insert(view_id, View::new(buffer));
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

        self.create_view(buffer_id)
    }

    pub fn get_open_buffers(&self) -> impl Iterator<Item = BufferId> + '_ {
        self.buffers.keys().copied()
    }

    pub fn active_view(&self) -> ViewId {
        self.active_view
    }

    pub fn view(&self, id: ViewId) -> Option<&View> {
        self.views.get(&id)
    }

    pub fn key_event(&self, key: char) {
        error!("{key:?}");
    }
}
