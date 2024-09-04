use std::{cell::RefCell, clone, rc::Rc};

use mlua::{FromLua, UserData};
use ropey::Rope;

use crate::{
    buffer::{Buffer, BufferId}, engine::{self, Engine}, selection::Selection, view::{View, ViewId}
};

pub fn init_lua(engine: Engine) -> anyhow::Result<()> {
    let lua = engine.state.borrow().lua;

    lua.set_app_data(engine.clone());

    let engine_table = lua.create_table()?;

    macro_rules! fix_type {
        ($ty:ty) => {
            $ty
        };
        () => {
            _
        };
    }

    macro_rules! method {
        (fn $name:ident $($t:tt)*) => {
            method!(fn (stringify!($name)) $($t)*)
        };
        ($(!)? fn ($name:expr) ($e:pat_param $(, $t:tt $(: $ty:ty )?)* $(,)?) $body:block) => {
            engine_table.raw_set(
                $name, lua.create_function(move |#[allow(unused_variables)] lua, ($( $t ,)*) : ($( fix_type!($($ty)?) ,)*)| {
                    let $e: Engine = lua.app_data_ref::<Engine>().unwrap().clone();
                    try { $body }
                })?
            )?;
        };
    }

    macro_rules! methods {
        (fn $a:tt $b:tt $c:tt $($rest:tt)*) => {
            method!(fn $a $b $c);
            methods!($($rest)*);
        };
        ($name:ident $(,)? $(;)? $($rest:tt)*) => {
            method!($name);
            methods!($($rest)*);
        };
        () => {};
    }

    // method!(fn create_new_buffer, |mut e| {
    //     let buffer = Buffer::create_from_contents("*scratch*".into(), Rope::new());
    //     let buffer_id = buffer.id;
    //     e.buffers.insert(buffer.id, buffer);

    //     BufferRef { id: buffer_id }
    // });

    // method!(create_view_for_buffer, |e, id: BufferId| {
    //     let view = View::new(buffer);
    // });

    fn register_command(e: Engine, args: mlua::MultiValue<'static>) -> mlua::Result<()> {
        let mut arg_count = args.len();
        let mut args = args.into_iter();
        let (name, func) = match arg_count {
            0 => return Err(mlua::Error::runtime("missing argument"))?,
            1 => {
                // let func = values[0].as_function().ok_or(mlua::Error::runtime("single argument given to register_command must be a named function"))?;
                let func =
                    match args.next().unwrap() {
                        mlua::Value::Function(func) => func,
                        _ => return Err(mlua::Error::runtime(
                            "single argument given to register command must be a named function",
                        ))?,
                    };
                let Some(name) = func.info().name else {
                    return Err(mlua::Error::runtime(
                        "single argument given to register command must be a named function",
                    ))?;
                };
                (name, func.clone())
            }
            2.. => {
                let name = match args.next().unwrap() {
                    mlua::Value::String(name) => name,
                    _ => {
                        return Err(mlua::Error::runtime(
                            "first argument given to register_command must be a string",
                        ))?
                    }
                };
                let func = match args.next().unwrap() {
                    mlua::Value::Function(func) => func,
                    _ => {
                        return Err(mlua::Error::runtime(
                            "second argument given to register_command must be a function",
                        ))?
                    }
                };
                (name.to_str()?.to_string(), func.clone())
            }
        };

        let mut state = e.state_mut();
        if state.commands.contains_key(&name) {
            return Err(mlua::Error::runtime(format!(
                "command {name} already exists"
            )))?;
        }
        state
            .commands
            .insert(name, crate::command::Command::Custom(func));

        Ok(())
    }

    engine_table.raw_set(
        "register_command",
        lua.create_function(move |lua, args: mlua::MultiValue| {
            register_command(lua.app_data_ref::<Engine>().unwrap().clone(), args)
        })?,
    )?;

    fn bind_key(e: Engine, key: char, command: String) -> mlua::Result<()> {
        e.state_mut().keybinds.binds.insert(key, command);
        Ok(())
    }

    engine_table.raw_set(
        "bind_key",
        lua.create_function(move |lua, (key, command): (String, String)| {
            bind_key(
                lua.app_data_ref::<Engine>().unwrap().clone(),
                key.chars().next().unwrap(),
                command,
            )
        })?,
    )?;

    methods! {

        fn open_file(e, path: String) {
            e.open(path);
        }

        fn create_buffer(e) {
            let id = e.create_buffer();
            BufferRef { id }
        }

        fn create_view_for_buffer(e, buffer_ref: BufferRef) {
            let id = e.create_view(buffer_ref.id);
            ViewRef { id }
        }

        fn set_active_view(e, view_ref: ViewRef) {
            e.state_mut().active_view = view_ref.id;
        }

        fn get_active_view(e) {
            ViewRef { id: e.active_view() }
        }

        fn get_views(e) {
            let views = e.state().views.keys().copied().map(|id| ViewRef { id }).collect::<Vec<_>>();
            views
        }
    }

    lua.globals().raw_set("Editor", engine_table)?;

    Ok(())
}

#[derive(Clone, Copy)]
pub struct BufferRef {
    id: BufferId,
}

impl UserData for BufferRef {
    fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("id", |_, buffer_ref| Ok(buffer_ref.id.0))
    }

    fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {}
}

impl<'lua> FromLua<'lua> for BufferRef {
    fn from_lua(value: mlua::Value<'lua>, lua: &'lua mlua::Lua) -> mlua::Result<Self> {
        Ok(*value
            .as_userdata()
            .ok_or(mlua::Error::runtime("oh noes"))?
            .borrow()?)
    }
}

#[derive(Clone, Copy)]
pub struct ViewRef {
    id: ViewId,
}

impl UserData for ViewRef {
    fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("id", |_, view_ref| Ok(view_ref.id.0))
    }

    fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {
        methods.add_method("get_selections", |lua, view_ref, ()| {
            let engine = lua.engine();
            let view = engine.view(view_ref.id).ok_or(mlua::Error::runtime("no view found for view id"))?;
            Ok(view.selections.clone())
        });

        methods.add_method("set_selections", |lua, view_ref, selections: Vec<Selection>| {
            let engine = lua.engine();
            let mut state = engine.state_mut();
            let view = state.views.get_mut(&view_ref.id).unwrap();
            view.selections = selections;

            Ok(())
        });
    }
}

impl<'lua> FromLua<'lua> for ViewRef {
    fn from_lua(value: mlua::Value<'lua>, lua: &'lua mlua::Lua) -> mlua::Result<Self> {
        Ok(*value
            .as_userdata()
            .ok_or(mlua::Error::runtime("oh noes"))?
            .borrow()?)
    }
}

pub trait GetEngine {
    fn engine(&self) -> Engine;
}

impl<'lua> GetEngine for &'lua mlua::Lua {
    fn engine(&self) -> Engine {
        self.app_data_ref::<Engine>().unwrap().clone()
    }
}
