use std::{
    io::Write,
    sync::atomic::{AtomicUsize, Ordering},
};

use log::debug;
use mlua::{FromLua, UserData};
use ratatui::style::Color;
use ropey::{Rope, RopeSlice};
use tree_sitter::{Parser, Tree};
use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

use crate::history::History;

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
        let mut highlighter = Highlighter::new();
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

        let highlights = highlighter
            .highlight(&config, content.as_bytes(), None, |_| None)
            .unwrap();

        let mut colors: Vec<Color> = vec![Color::White; content.len()];

        let mut color_stack: Vec<Color> = Vec::new();

        for event in highlights {
            match event.unwrap() {
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

        Self {
            id,
            name,
            view_count: 0,
            contents: rope,
            history: History::new(),
            backing: BufferBacking::None,
            parser,
            tree,
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
