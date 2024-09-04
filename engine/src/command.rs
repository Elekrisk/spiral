use std::{cell::RefCell, rc::Rc};

use crate::engine::Engine;


pub enum Command {
    Builtin(BuiltinCommand),
    Custom(mlua::Function<'static>)
}

pub struct BuiltinCommand {
    pub name: String,
    pub action: Box<dyn Fn(Rc<RefCell<Engine>>, Vec<String>)>
}



pub fn builtin_commands() -> Vec<BuiltinCommand> {
    vec![
        BuiltinCommand {
            name: "open".into(),
            action: Box::new(|e, args| {
                e.borrow_mut().open(&args[0]);
            })
        }
    ]
}