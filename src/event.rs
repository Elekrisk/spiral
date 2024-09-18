use mlua::UserData;

use crate::mode::Mode;

#[derive(Debug, Clone)]
pub struct Event {
    pub kind: EventKind,
}

impl UserData for Event {
    fn add_fields<'lua, F: mlua::UserDataFields<'lua, Self>>(fields: &mut F) {
        fields.add_field_method_get("kind", |_, e| {
            Ok(match &e.kind {
                EventKind::ModeTransition { .. } => "mode-transition",
            })
        });

        // ModeTransition
        fields.add_field_method_get("old_mode", |_, e| match &e.kind {
            EventKind::ModeTransition { old, .. } => Ok(old.to_string()),
        });
        fields.add_field_method_get("new_mode", |_, e| match &e.kind {
            EventKind::ModeTransition { new, .. } => Ok(new.to_string()),
        });
    }

    fn add_methods<'lua, M: mlua::UserDataMethods<'lua, Self>>(methods: &mut M) {}
}

#[derive(Debug, Clone)]
pub enum EventKind {
    ModeTransition { old: Mode, new: Mode },
}
