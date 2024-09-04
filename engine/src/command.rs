use std::{cell::RefCell, rc::Rc};

use crate::engine::Engine;


pub enum Command {
    Builtin(BuiltinCommand),
    Custom(mlua::Function<'static>)
}

pub trait CommandAction {
    fn call(&self, engine: Engine, args: Vec<String>);
    fn boxed_clone(&self) -> Box<dyn CommandAction>;
}

impl<F: Fn(Engine, Vec<String>) + Clone + 'static> CommandAction for F {
    fn call(&self, engine: Engine, args: Vec<String>) {
        self(engine, args)
    }

    fn boxed_clone(&self) -> Box<dyn CommandAction> {
        Box::new(self.clone())
    }
}

impl Clone for Box<dyn CommandAction> {
    fn clone(&self) -> Self {
        self.boxed_clone()
    }
}

pub struct BuiltinCommand {
    pub name: String,
    pub action: Box<dyn CommandAction>
}


pub fn builtin_commands() -> Vec<BuiltinCommand> {
    vec![
        BuiltinCommand {
            name: "open".into(),
            action: Box::new(|e: Engine, args: Vec<String>| {
                e.open(&args[0]);
            })
        },
        BuiltinCommand {
            name: "new-buffer".into(),
            action: Box::new(|e: Engine, args: Vec<String>| {
                let id = e.create_buffer();
                let id = e.create_view(id);
                e.state_mut().active_view = id;
            })
        }
    ]
}