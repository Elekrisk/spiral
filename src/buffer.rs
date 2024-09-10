use std::{
    io::Write,
    sync::atomic::{AtomicUsize, Ordering},
};

use log::debug;
use mlua::{FromLua, UserData};
use ratatui::style::Color;
use ropey::{Rope, RopeSlice};
use tree_sitter::{InputEdit, Parser, Point, Tree};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use crate::view::View;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BufferId(pub usize);

impl BufferId {
    pub fn generate() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        Self(id)
    }
}

impl<'lua> FromLua<'lua> for BufferId {
    fn from_lua(value: mlua::Value<'lua>, lua: &'lua mlua::Lua) -> mlua::Result<Self> {
        Ok(*value
            .as_userdata()
            .ok_or_else(|| mlua::Error::runtime("oh noes"))?
            .borrow()?)
    }
}

pub struct Buffer {
    pub id: BufferId,
    pub name: String,
    pub view_count: usize,
    pub contents: ropey::Rope,
    pub history: History,

    pub backing: BufferBacking,

    pub parser: Parser,
    pub tree: Tree,
    pub highlighter: HighlightCtx,

    pub colors: Vec<Color>,
}

impl Buffer {
    pub fn create_from_contents(name: String, rope: Rope) -> Self {
        let id = BufferId::generate();

        let content = rope.to_string();

        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_rust::language())
            .expect("Error loading Rust grammar");

        let tree = parser.parse(&content, None).unwrap();

        let highlight_names = [
            "keyword", "function", "type", "number", "string", "variable",
        ];
        let highlighter = Highlighter::new();
        let rust_language = tree_sitter_rust::language();
        let mut config = HighlightConfiguration::new(
            rust_language,
            "rust",
            tree_sitter_rust::HIGHLIGHTS_QUERY,
            tree_sitter_rust::INJECTIONS_QUERY,
            "",
        )
        .unwrap();
        config.configure(&highlight_names);

        let mut highlighter = HighlightCtx {
            highlighter,
            config,
        };

        let colors = highlighter.highlight(rope.to_string().as_bytes()).unwrap();

        Self {
            id,
            name,
            view_count: 0,
            history: History::new(),
            backing: BufferBacking::None,
            parser,
            tree,
            highlighter,
            contents: rope,
            colors,
        }
    }

    pub fn set_backing(&mut self, backing: BufferBacking) {
        self.backing = backing;
    }

    pub fn get_visible_part(&self, top_line: usize, mut line_count: usize) -> Option<RopeSlice> {
        if self.contents.len_lines() < top_line {
            None
        } else {
            line_count = line_count.min(self.contents.len_lines() - top_line);
            let first_line = top_line;
            let last_line = top_line + line_count - 1;
            let first_char = self.contents.line_to_char(first_line);
            let last_char = self.contents.line_to_char(last_line + 1);
            Some(self.contents.slice(first_char..last_char))
        }
    }

    pub fn insert(&mut self, view: &mut View, text: &str, char_index: usize) {
        let char_index = char_index.min(self.contents.len_chars());

        let byte_start = self.contents.char_to_byte(char_index);
        let line_start = self.contents.byte_to_line(byte_start);
        let col_start = byte_start - self.contents.line_to_byte(line_start);
        let text_len = text.len();
        let newline_count = text.bytes().filter(|c| *c == b'\n').count();
        let after_last_newline = text.split('\n').next_back().unwrap().len();

        let input_edit = InputEdit {
            start_byte: byte_start,
            old_end_byte: byte_start,
            new_end_byte: byte_start + text_len,
            start_position: Point::new(line_start, col_start),
            old_end_position: Point::new(line_start, col_start),
            new_end_position: Point::new(
                line_start + newline_count,
                if newline_count == 0 { col_start } else { 0 } + after_last_newline,
            ),
        };

        self.tree.edit(&input_edit);

        self.contents.insert(char_index, text);

        let start = char_index;
        let char_len = text.chars().count();

        for selection in &mut view.selections {
            if selection.start >= start {
                selection.start += char_len;
            }
            if selection.end >= start {
                selection.end += char_len;
            }
        }
    }

    pub fn remove(&mut self, view: &mut View, char_index: usize, len: usize) {
        let char_index = char_index.min(self.contents.len_chars());
        let len = len.min(self.contents.len_chars() - char_index);

        let byte_start = self.contents.char_to_byte(char_index);
        let byte_end = self.contents.char_to_byte(char_index + len);
        let line_start = self.contents.byte_to_line(byte_start);
        let col_start = byte_start - self.contents.line_to_byte(line_start);
        let line_end = self.contents.byte_to_line(byte_end);
        let col_end = byte_end - self.contents.line_to_byte(line_end);

        let input_edit = InputEdit {
            start_byte: byte_start,
            old_end_byte: byte_end,
            new_end_byte: byte_start,
            start_position: Point::new(line_start, col_start),
            old_end_position: Point::new(line_end, col_end),
            new_end_position: Point::new(line_start, col_start),
        };

        self.tree.edit(&input_edit);

        self.contents.remove(char_index..char_index + len);

        let start = char_index;

        for selection in &mut view.selections {
            if selection.start >= start {
                selection.start = (selection.start.saturating_sub(len)).max(start);
            }
            if selection.end >= start {
                selection.end = (selection.end.saturating_sub(len)).max(start);
            }
        }
    }

    pub fn recalc_tree(&mut self) {
        let contents = self.contents.to_string();
        self.tree = self.parser.parse(&contents, Some(&self.tree)).unwrap();
        self.colors = self.highlighter.highlight(contents.as_bytes()).unwrap();
    }

    pub fn undo(&mut self, view: &mut View) {
        let mut history = std::mem::take(&mut self.history);
        if let Some(action) = history.back() {
            for action in &action.actions {
                match action {
                    Action::TextInsertion { text, start } => {
                        self.remove(view, *start, text.chars().count());
                    }
                    Action::TextDeletion {
                        deleted_text,
                        start,
                        len: _,
                    } => {
                        self.insert(view, deleted_text, *start);
                    }
                }
            }
            self.recalc_tree();
            view.merge_overlapping_selections();
            view.make_selection_visisble(self);
        }
        self.history = history;
    }

    pub fn redo(&mut self, view: &mut View) {
        let mut history = std::mem::take(&mut self.history);
        if let Some(action) = history.forward() {
            for action in &action.actions {
                match action {
                    Action::TextInsertion { text, start } => self.insert(view, text, *start),
                    Action::TextDeletion {
                        deleted_text: _,
                        start,
                        len,
                    } => {
                        self.remove(view, *start, *len);
                    }
                }
            }
            self.recalc_tree();
            view.merge_overlapping_selections();
            view.make_selection_visisble(self);
        }
        self.history = history;
    }
}

pub enum BufferBacking {
    None,
    File(std::path::PathBuf),
}

impl BufferBacking {
    pub fn save(&self, buffer: &Buffer) -> anyhow::Result<()> {
        match self {
            BufferBacking::None => Ok(()),
            BufferBacking::File(path) => {
                let mut writer = std::fs::File::create(path)?;
                for chunk in buffer.contents.chunks() {
                    writer.write_all(chunk.as_bytes())?;
                }

                Ok(())
            }
        }
    }
}

pub struct HighlightCtx {
    pub highlighter: Highlighter,
    pub config: HighlightConfiguration,
}

impl HighlightCtx {
    pub fn highlight(&mut self, text: &[u8]) -> anyhow::Result<Vec<Color>> {
        let highlights = self
            .highlighter
            .highlight(&self.config, text, None, |_| None)?;

        let mut colors: Vec<Color> = vec![Color::White; text.len()];

        let mut color_stack: Vec<Color> = Vec::new();

        for event in highlights {
            match event? {
                // Processed a chunk of text spanning from start..end
                HighlightEvent::Source { start, end } => {
                    // Sometimes you will get a source event that has no highlight,
                    // so make sure to check if there is a color on the stack
                    if let Some(color) = color_stack.last() {
                        (start..end).for_each(|i| {
                            colors[i] = *color;
                        });
                    }
                }
                HighlightEvent::HighlightStart(highlight) => {
                    // `highlight` is a tuple struct containing the node type's ID
                    let node_type_id = highlight.0;
                    color_stack.push(match node_type_id {
                        0 => Color::Red,
                        1 => Color::Blue,
                        2 => Color::Yellow,
                        3 => Color::Magenta,
                        4 => Color::Green,
                        5 => Color::Cyan,
                        _ => Color::White,
                    });
                }
                HighlightEvent::HighlightEnd => {
                    color_stack.pop();
                }
            }
        }

        Ok(colors)
    }
}

#[derive(Default)]
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

    pub fn back(&mut self) -> Option<&HistoryAction> {
        if self.cursor > 0 {
            self.cursor -= 1;
            Some(&self.actions[self.cursor])
        } else {
            None
        }
    }

    pub fn forward(&mut self) -> Option<&HistoryAction> {
        if self.cursor < self.actions.len() {
            self.cursor += 1;
            Some(&self.actions[self.cursor - 1])
        } else {
            None
        }
    }
}

pub struct HistoryAction {
    pub actions: Vec<Action>,
}

pub enum Action {
    TextInsertion {
        text: String,
        start: usize,
    },
    TextDeletion {
        deleted_text: String,
        start: usize,
        len: usize,
    },
}
