#![feature(panic_update_hook)]

use std::{cell::RefCell, io::stdout, path::PathBuf, rc::Rc, time::Duration};

use clap::Parser;
use engine::{buffer::Buffer, selection::Selection, view::View, engine::Engine};
use env_logger::Target;
use ratatui::{
    backend::CrosstermBackend,
    crossterm::{
        event::{self, KeyCode, KeyModifiers},
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
        ExecutableCommand,
    },
    style::Modifier,
    widgets::Widget,
    Frame, Terminal,
};

#[derive(Parser)]
pub struct Options {
    path: Option<PathBuf>,
}

pub struct Frontend {
    engine: Engine,
    exit: bool,
}

impl Frontend {
    fn handle_events(&mut self) {
        if event::poll(Duration::from_millis(100)).unwrap() {
            match event::read().unwrap() {
                event::Event::Key(key) => match key.code {
                    KeyCode::Char('q') if key.modifiers == KeyModifiers::CONTROL => self.exit = true,
                    KeyCode::Char(other) => self.engine.key_event(other),
                    _ => {}
                },
                _ => {}
            }
        }
    }

    fn draw(&self, frame: &mut Frame) {
        let active_view = self.engine.active_view();
        let view = self.engine.view(active_view).unwrap();
        let buffer = self.engine.buffer(view.buffer).unwrap();
        frame.render_widget(
            ViewWidget {
                view: &view,
                buffer: &buffer,
                selections: &view.selections,
            },
            frame.area(),
        );
    }
}

struct ViewWidget<'a> {
    view: &'a View,
    buffer: &'a Buffer,
    selections: &'a [Selection],
}

fn fixed_char(char: char) -> char {
    if char.is_control() {
        ' '
    } else {
        char
    }
}

impl<'a> Widget for ViewWidget<'a> {
    fn render(self, area: ratatui::prelude::Rect, buf: &mut ratatui::prelude::Buffer)
    where
        Self: Sized,
    {
        let text = self
            .buffer
            .get_visible_part(0, area.height as usize)
            .unwrap();
        for (row, line) in text.lines().enumerate() {
            for (col, char) in line.chars().take(area.width as usize).enumerate() {
                buf.cell_mut((col as u16, row as u16))
                    .unwrap()
                    .set_char(fixed_char(char));
            }
        }

        for selection in self.selections {
            log::error!("{selection:?}");
            let start_line = self.buffer.contents.char_to_line(selection.start);
            let mut start_col = selection.start - self.buffer.contents.line_to_char(start_line);
            let mut rel_start_line = start_line as isize - self.view.scroll as isize;

            let end_line = self.buffer.contents.char_to_line(selection.end);
            let mut end_col = selection.end - self.buffer.contents.line_to_char(end_line);
            let mut rel_end_line = end_line as isize - self.view.scroll as isize;

            if (rel_start_line < 0 && rel_end_line < 0)
                || (rel_start_line >= area.height as isize && rel_end_line >= area.height as isize)
            {
                continue;
            }

            if rel_start_line < 0 {
                rel_start_line = 0;
                start_col = 0;
            }

            if rel_end_line >= area.height as isize {
                rel_end_line = area.height as isize - 1;
                end_col = text.line(area.height as usize - 1).len_chars() - 1;
            }

            if rel_start_line == rel_end_line {
                for col in start_col..=end_col {
                    buf.cell_mut((col as u16, rel_start_line as u16))
                        .unwrap()
                        .modifier
                        .insert(Modifier::REVERSED);
                }
            } else {
                for col in start_col..area.width.min(text.line(rel_start_line as usize).len_chars() as u16) as usize {
                    buf.cell_mut((col as u16, rel_start_line as u16))
                        .unwrap()
                        .modifier
                        .insert(Modifier::REVERSED);
                }
                for row in rel_start_line + 1..rel_end_line {
                    for col in 0..area.width.min(text.line(row as usize).len_chars() as u16) as usize {
                        buf.cell_mut((col as u16, row as u16))
                            .unwrap()
                            .modifier
                            .insert(Modifier::REVERSED);
                    }
                }
                for col in 0..=end_col {
                    buf.cell_mut((col as u16, rel_end_line as u16))
                        .unwrap()
                        .modifier
                        .insert(Modifier::REVERSED);
                }
            }
        }
    }
}

fn main() {
    env_logger::Builder::from_default_env()
        .target(Target::Pipe(Box::new(
            std::fs::File::create("./log.log").unwrap(),
        )))
        .init();

    let options = Options::parse();
    let engine = Engine::new().unwrap();

    engine.load_lua("./config.lua");

    if let Some(path) = options.path {
        engine.open(path);
    }

    let mut frontend = Frontend {
        engine,
        exit: false,
    };

    std::panic::update_hook(|hook, info| {
        let _ = disable_raw_mode();
        let _  =stdout().execute(LeaveAlternateScreen);
        hook(info)
    });

    enable_raw_mode().unwrap();
    stdout().execute(EnterAlternateScreen).unwrap();
    let mut terminal = Terminal::new(CrosstermBackend::new(stdout())).unwrap();

    while !frontend.exit {
        frontend.handle_events();
        terminal.draw(|frame| frontend.draw(frame)).unwrap();
    }

    disable_raw_mode().unwrap();
    stdout().execute(LeaveAlternateScreen).unwrap();
}
