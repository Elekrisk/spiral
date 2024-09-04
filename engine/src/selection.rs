use log::error;
use mlua::{FromLua, UserData};

use crate::{lua::GetEngine, view::ViewId};

#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub view: ViewId,
    pub start: usize,
    pub end: usize,
}

impl Selection {
    pub fn new(view: ViewId) -> Self {
        Self { view, start: 0, end: 0 }
    }
}

impl UserData for Selection {
    fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("start", |_, s| Ok(s.start));
        fields.add_field_method_set("start", |_, s, val: usize| {
            s.start = val;
            Ok(())
        });
        fields.add_field_method_get("end", |_, s| Ok(s.end));
        fields.add_field_method_set("end", |_, s, val: usize| {
            s.end = val;
            Ok(())
        });
    }

    fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("get_text", |lua, selection, ()| {
            let engine = lua.engine();
            let state = engine.state();
            let view = state.view(selection.view).unwrap();
            let buffer = state.buffer(view.buffer).unwrap();
            let text = buffer.contents.slice(selection.start..=selection.end);

            Ok(text.to_string())
        });
        methods.add_method("set_text", |lua, selection, ()| {
            let engine = lua.engine();
            let mut state = engine.state_mut();
            error!("{}", selection.view.0);
            for (k, v) in &state.views {
                error!("{} -> {}", k.0, v.id.0);
            }
            let view = state.view(selection.view).unwrap();
            let buffer_id = view.buffer;
            let buffer = state.buffers.get_mut(&buffer_id).unwrap();
            buffer.contents.remove(selection.start..=selection.end);
            let text = buffer.contents.slice(selection.start..=selection.end);

            Ok(text.to_string())
        });
    }
}

impl<'lua> FromLua<'lua> for Selection {
    fn from_lua(value: mlua::Value<'lua>, lua: &'lua mlua::Lua) -> mlua::Result<Self> {
        Ok(*value.as_userdata().ok_or(mlua::Error::runtime("oh noes"))?.borrow()?)
    }
}
