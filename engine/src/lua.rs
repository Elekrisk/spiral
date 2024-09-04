use std::{cell::RefCell, rc::Rc};

use mlua::UserData;
use ropey::Rope;

use crate::{
    buffer::{Buffer, BufferId},
    engine::Engine,
    view::{View, ViewId},
};

pub fn init_lua(engine: Rc<RefCell<Engine>>) -> anyhow::Result<()> {
    let e = engine.clone();
    let engine = engine.borrow();
    let lua = &engine.lua;

    let engine_table = engine.lua.create_table()?;

    macro_rules! fix_type {
        ($ty:ty) => {$ty};
        () => {_};
    }

    macro_rules! method {
        (fn $name:ident $($t:tt)*) => {
            method!(fn (stringify!($name)) $($t)*)
        };
        (fn ($name:expr) ($e:ident $(, $($t:tt)*)?) $body:block) => {
            method!(! fn ($name) ($e $(, $($t)*)?) { let $e = $e.borrow(); $body });
        };
        (fn ($name:expr) (mut $e:ident $(, $($t:tt)*)?) $body:block) => {
            method!(! fn ($name) ($e $(, $($t)*)?) { let mut $e = $e.borrow_mut(); $body });
        };
        ($(!)? fn ($name:expr) ($e:pat_param $(, $t:tt $(: $ty:ty )?)* $(,)?) $body:block) => {
            {
                let $e = e.clone();
                engine_table.raw_set(
                    $name, lua.create_function(move |#[allow(unused_variables)] lua, ($( $t ),*) : ($(fix_type!($($ty)?)),*)| try { $body })?
                )?;
            }
        };
    }

    macro_rules! methods {
        (fn $a:tt $b:tt $c:tt $($rest:tt)*) => {
            method!(fn $a $b $c);
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

    methods! {
        fn create_new_buffer(mut e,) {
            let id = e.create_buffer();
            BufferRef { id }
        }

        fn create_view_for_buffer(mut e, buffer_id: BufferId) {
            let id = e.create_view(buffer_id);
        }

        fn set_active_view(mut e, view_id: ViewId) {
            e.active_view = view_id;
        }
    }

    Ok(())
}

pub struct BufferRef {
    id: BufferId,
}

impl UserData for BufferRef {
    fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("id", |_, buffer_ref| Ok(buffer_ref.id.get_usize()))
    }

    fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {}
}
