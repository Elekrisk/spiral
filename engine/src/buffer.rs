use std::sync::atomic::{AtomicUsize, Ordering};

use log::debug;
use mlua::{FromLua, UserData};
use ropey::{Rope, RopeSlice};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferId(pub usize);

impl BufferId {
    pub fn generate() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        Self(id)
    }
}

impl<'lua> FromLua<'lua> for BufferId {
    fn from_lua(value: mlua::Value<'lua>, lua: &'lua mlua::Lua) -> mlua::Result<Self> {
        Ok(*value
            .as_userdata()
            .ok_or_else(|| mlua::Error::runtime("oh noes"))?
            .borrow()?)
    }
}

pub struct Buffer {
    pub id: BufferId,
    pub name: String,
    pub contents: ropey::Rope,
}

impl Buffer {
    pub fn create_from_contents(name: String, rope: Rope) -> Self {
        let id = BufferId::generate();
        Self {
            id,
            name,
            contents: rope,
        }
    }

    pub fn get_visible_part(&self, top_line: usize, mut line_count: usize) -> Option<RopeSlice> {
        if self.contents.len_lines() < top_line {
            None
        } else {
            line_count = line_count.min(self.contents.len_lines() - top_line);
            let first_line = top_line;
            let last_line = top_line + line_count - 1;
            let first_char = self.contents.line_to_char(first_line);
            let last_char = self.contents.line_to_char(last_line + 1);
            Some(self.contents.slice(first_char..last_char))
        }
    }
}
