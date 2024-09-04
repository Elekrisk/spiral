use std::sync::atomic::{AtomicUsize, Ordering};

use mlua::FromLua;

use crate::{buffer::BufferId, selection::Selection};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewId(usize);

impl ViewId {
    pub fn generate() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        Self(id)
    }
}

impl<'lua> FromLua<'lua> for ViewId {
    fn from_lua(value: mlua::Value<'lua>, lua: &'lua mlua::Lua) -> mlua::Result<Self> {
        Ok(*value
            .as_userdata()
            .ok_or_else(|| mlua::Error::runtime("oh noes"))?
            .borrow()?)
    }
}

pub struct View {
    pub id: ViewId,
    pub buffer: BufferId,
    pub scroll: usize,
    pub selections: Vec<Selection>,
}

impl View {
    pub fn new(buffer: BufferId) -> Self {
        let id = ViewId::generate();
        Self {
            id,
            buffer,
            scroll: 0,
            selections: vec![Selection::new()],
        }
    }
}
