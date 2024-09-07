use ropey::Rope;

use crate::{buffer::Buffer, engine::EngineState};


pub struct History {
    actions: Vec<HistoryAction>,
    cursor: usize,
}

impl History {
    pub fn new() -> Self {
        Self {
            actions: vec![],
            cursor: 0,
        }
    }

    pub fn register_edit(&mut self, edits: HistoryAction) {
        self.actions.truncate(self.cursor);
        self.actions.push(edits);
        self.cursor += 1;
    }

    pub fn undo(&mut self, contents: &mut Rope) {
        if self.cursor == 0 {
            return;
        }
        let action = &self.actions[self.cursor - 1];
        for action in action.actions.iter().rev() {
            action.undo(contents);
        }
        self.cursor -= 1;
    }

    pub fn redo(&mut self, contents: &mut Rope) {
        if self.cursor == self.actions.len() { return }
        let action = &self.actions[self.cursor];
        for action in action.actions.iter().rev() {
            action.redo(contents);
        }
        self.cursor += 1;
    }
}

pub struct HistoryAction {
    pub actions: Vec<Action>
}

pub enum Action {
    TextInsertion {
        text: String,
        start: usize,
    },
    TextDeletion {
        deleted_text: String,
        start: usize,
        end: usize,
    }
}

impl Action {
    pub fn undo(&self, rope: &mut Rope) {
        match self {
            Action::TextInsertion {
                text, start
            } => {
                let end = start + text.len();
                rope.remove(*start..end);
            },
            Action::TextDeletion { deleted_text, start, end } => {
                rope.insert(*start, deleted_text);
            },
        }
    }

    pub fn redo(&self, rope: &mut Rope) {
        match self {
            Action::TextInsertion { text, start } => {
                rope.insert(*start, text);
            },
            Action::TextDeletion { deleted_text, start, end } => {
                rope.remove(*start..*end);
            },
        }
    }
}
