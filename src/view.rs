use std::sync::atomic::{AtomicUsize, Ordering};

use mlua::FromLua;
use ratatui::{
    style::{Color, Modifier, Style},
    text::ToText,
    widgets::Widget,
};
use ropey::Rope;

use crate::{
    buffer::{Buffer, BufferId},
    engine::Size,
    selection::Selection,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ViewId(pub usize);

impl ViewId {
    pub fn generate() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(1);
        let id = NEXT.fetch_add(1, Ordering::Relaxed);
        Self(id)
    }
}

impl<'lua> FromLua<'lua> for ViewId {
    fn from_lua(value: mlua::Value<'lua>, lua: &'lua mlua::Lua) -> mlua::Result<Self> {
        Ok(*value
            .as_userdata()
            .ok_or_else(|| mlua::Error::runtime("oh noes"))?
            .borrow()?)
    }
}

pub struct View {
    pub id: ViewId,
    pub buffer: BufferId,
    pub vscroll: usize,
    pub hscroll: usize,

    pub size: Size,

    pub selections: Vec<Selection>,
}

impl View {
    pub fn new(buffer: BufferId, size: Size) -> Self {
        let id = ViewId::generate();
        Self {
            id,
            buffer,
            vscroll: 0,
            hscroll: 0,
            size,
            selections: vec![Selection::new(id)],
        }
    }

    pub fn resize(&mut self, size: Size) {
        self.size = size;
    }

    pub fn make_selection_visisble(&mut self, buffer: &Buffer) {
        let Some(primary) = self.selections.first() else {
            return;
        };
        let head = primary.head();
        let line = buffer.contents.char_to_line(head);

        if line < self.vscroll {
            self.vscroll = line;
        }

        if line >= self.vscroll + self.size.height {
            self.vscroll = line - self.size.height + 1;
        }
    }
}

pub struct ViewWidget<'a> {
    pub view: &'a View,
    pub buffer: &'a Buffer,
}

impl<'a> Widget for ViewWidget<'a> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let view = self.view;
        let buffer = self.buffer;

        let Some(lines) = buffer.contents.get_lines_at(view.vscroll) else {
            return;
        };
        let lines = lines.take(area.height as usize);

        for (row, line) in lines.enumerate() {
            buf.set_string(0, row as _, line.to_string(), Style::new());
        }

        let text = &buffer.contents;

        for selection in &self.view.selections {
            let start_char = selection.start;
            let start_line = text.char_to_line(start_char);
            let start_col = start_char - text.line_to_char(start_line);

            let end_char = selection.end;
            let end_line = text.char_to_line(end_char);
            let end_col = end_char - text.line_to_char(end_line);

            if start_line < view.vscroll && end_line < view.vscroll
                || start_line >= view.vscroll + area.height as usize
                    && end_line >= view.vscroll + area.height as usize
            {
                continue;
            }

            let clamped_start_line = start_line.max(view.vscroll);
            let clamped_end_line = end_line.min(view.vscroll + area.height as usize - 1);

            let clamped_start_col = if clamped_start_line == start_line {
                start_col.max(view.hscroll)
            } else {
                0
            };
            let clamped_end_col = if clamped_end_line == end_line {
                end_col.min(view.hscroll + view.size.width - 1)
            } else {
                usize::MAX
            };

            fn fill_range(
                buf: &mut ratatui::buffer::Buffer,
                line: usize,
                start: usize,
                end: usize,
            ) {
                for col in start..=end {
                    buf[(col as u16, line as u16)].bg = Color::DarkGray;
                }
            }

            let mut fill_range = |line, start: usize, end: usize, last_line: bool| {
                fill_range(
                    buf,
                    line - view.vscroll,
                    start.min(text.line(line).len_chars().saturating_sub(if last_line {
                        0
                    } else {
                        1
                    })) - view.hscroll,
                    end.min(text.line(line).len_chars().saturating_sub(if last_line {
                        0
                    } else {
                        1
                    })) - view.hscroll,
                )
            };

            if clamped_start_line == clamped_end_line {
                fill_range(clamped_start_line, clamped_start_col, clamped_end_col, true);
            } else {
                fill_range(clamped_start_line, clamped_start_col, usize::MAX, false);
                for line in clamped_start_line + 1..clamped_end_line {
                    fill_range(line, 0, usize::MAX, false);
                }
                fill_range(clamped_end_line, 0, clamped_end_col, true);
            }

            let head = selection.head();
            let head_line = text.char_to_line(head);
            let head_col = head - text.line_to_char(head_line);

            if head_line < view.vscroll
                || head_line >= view.vscroll + area.height as usize
                || head_col < view.hscroll
                || head_col >= view.hscroll + area.width as usize
            {
                continue;
            }

            buf[(
                (head_col - view.hscroll) as u16,
                (head_line - view.vscroll) as u16,
            )]
                .modifier
                .insert(Modifier::REVERSED);
        }
    }
}
