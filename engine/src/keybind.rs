use std::collections::HashMap;

use crate::command::Command;

pub struct Keybindings {
    pub binds: HashMap<char, String>,
}

pub fn default_keybinds() -> Keybindings {
    Keybindings {
        binds: [('n', "new-buffer".into())].into(),
    }
}
