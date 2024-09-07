use log::error;
use mlua::{FromLua, IntoLua, UserData};
use ropey::Rope;

use crate::{lua::GetEngine, view::ViewId};

#[derive(Debug, Clone, Copy)]
pub struct Selection {
    pub view: ViewId,
    pub start: usize,
    pub end: usize,
    pub dir: Direction,
}

impl Selection {
    pub fn new(view: ViewId) -> Self {
        Self {
            view,
            start: 0,
            end: 0,
            dir: Direction::Forward,
        }
    }

    pub fn head(&self) -> usize {
        match self.dir {
            Direction::Forward => self.end,
            Direction::Back => self.start,
        }
    }

    pub fn anchor(&self) -> usize {
        match self.dir {
            Direction::Forward => self.start,
            Direction::Back => self.end,
        }
    }

    pub fn head_mut(&mut self) -> &mut usize {
        match self.dir {
            Direction::Forward => &mut self.end,
            Direction::Back => &mut self.start,
        }
    }

    pub fn anchor_mut(&mut self) -> &mut usize {
        match self.dir {
            Direction::Forward => &mut self.start,
            Direction::Back => &mut self.end,
        }
    }

    pub fn head_anchor_mut(&mut self) -> (&mut usize, &mut usize) {
        match self.dir {
            Direction::Forward => (&mut self.end, &mut self.start),
            Direction::Back => (&mut self.start, &mut self.end),
        }
    }

    pub fn make_valid(&mut self, text: &Rope) {
        if self.start > self.end {
            std::mem::swap(&mut self.start, &mut self.end);
            self.dir = match self.dir {
                Direction::Forward => Direction::Back,
                Direction::Back => Direction::Forward,
            }
        }

        let len = text.len_chars();
        if self.start > len {
            self.start = len;
        }
        if self.end > len {
            self.end = len;
        }
    }
}

impl UserData for Selection {
    fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("start", |_, s| Ok(s.start));
        fields.add_field_method_set("start", |lua, s, val: usize| {
            let engine = lua.engine();
            let state = engine.state();
            let view_id = state.active_view;
            let view = state.view(view_id).unwrap();
            let buffer = state.buffer(view.buffer).unwrap();
            s.start = val;
            s.make_valid(&buffer.contents);
            Ok(())
        });
        fields.add_field_method_get("end", |_, s| Ok(s.end));
        fields.add_field_method_set("end", |lua, s, val: usize| {
            let engine = lua.engine();
            let state = engine.state();
            let view_id = state.active_view;
            let view = state.view(view_id).unwrap();
            let buffer = state.buffer(view.buffer).unwrap();
            s.end = val;
            s.make_valid(&buffer.contents);
            Ok(())
        });

        fields.add_field_method_get("head", |_, s| {
            Ok(match s.dir {
                Direction::Forward => s.end,
                Direction::Back => s.start,
            })
        });
        fields.add_field_method_set("head", |lua, s, val: usize| {
            let engine = lua.engine();
            let state = engine.state();
            let view_id = state.active_view;
            let view = state.view(view_id).unwrap();
            let buffer = state.buffer(view.buffer).unwrap();
            match s.dir {
                Direction::Forward => s.end = val,
                Direction::Back => s.start = val,
            }
            s.make_valid(&buffer.contents);
            Ok(())
        });
        fields.add_field_method_get("anchor", |_, s| {
            Ok(match s.dir {
                Direction::Forward => s.start,
                Direction::Back => s.end,
            })
        });
        fields.add_field_method_set("anchor", |lua, s, val: usize| {
            let engine = lua.engine();
            let state = engine.state();
            let view_id = state.active_view;
            let view = state.view(view_id).unwrap();
            let buffer = state.buffer(view.buffer).unwrap();
            match s.dir {
                Direction::Forward => s.start = val,
                Direction::Back => s.end = val,
            }
            s.make_valid(&buffer.contents);
            Ok(())
        });

        fields.add_field_method_get("direction", |_, s| Ok(s.dir));
        fields.add_field_method_set("direction", |_, s, dir: Direction| {
            s.dir = dir;
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
        Ok(*value
            .as_userdata()
            .ok_or(mlua::Error::runtime("oh noes"))?
            .borrow()?)
    }
}

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Forward,
    Back,
}

impl<'lua> IntoLua<'lua> for Direction {
    fn into_lua(self, lua: &'lua mlua::Lua) -> mlua::Result<mlua::Value<'lua>> {
        lua.create_string(match self {
            Direction::Forward => "forward",
            Direction::Back => "back",
        })
        .map(mlua::Value::String)
    }
}

impl<'lua> FromLua<'lua> for Direction {
    fn from_lua(value: mlua::Value<'lua>, lua: &'lua mlua::Lua) -> mlua::Result<Self> {
        match value.as_str().ok_or(mlua::Error::runtime("oh noes"))? {
            "forward" => Ok(Self::Forward),
            "back" => Ok(Self::Back),
            _ => Err(mlua::Error::runtime("invalid selection direction")),
        }
    }
}
