use std::collections::HashMap;

use crate::command::Command;


pub struct Keybindings {
    pub binds: HashMap<char, Command>
}
