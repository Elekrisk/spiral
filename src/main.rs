#![feature(try_blocks)]
#![feature(macro_metavar_expr)]
#![feature(iterator_try_collect)]
#![feature(panic_update_hook)]
#![feature(let_chains)]
#![feature(iter_intersperse)]

mod buffer;
mod command;
mod engine;
mod keybind;
mod lua;
mod mode;
mod selection;
mod view;
mod history;
mod kill_ring;

use std::{
    collections::HashMap,
    fs::File,
    io::stdout,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

use buffer::{Buffer, BufferId};
use clap::Parser;
use engine::Engine;
use log::{debug, error, warn};
use ratatui::{
    crossterm::{
        event::{
            self, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
            PushKeyboardEnhancementFlags,
        },
        terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
        ExecutableCommand,
    },
    prelude::CrosstermBackend,
    Terminal,
};
use ropey::Rope;
use view::{View, ViewId};

#[derive(clap::Parser)]
struct Options {
    path: Option<PathBuf>,
}

fn main() {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Trace)
        .target(env_logger::Target::Pipe(Box::new(
            std::fs::File::create("./log.log").unwrap(),
        )))
        .init();

    let options = Options::parse();

    let engine = Engine::new().unwrap();
    if let Err(e) = engine.reload_config() {
        eprintln!("{e}");
        return;
    }
    if let Some(path) = options.path {
        engine.open(path);
    }

    let mut terminal = Terminal::new(CrosstermBackend::new(stdout())).unwrap();

    std::panic::update_hook(|hook, info| {
        let _ = disable_raw_mode();
        let _ = stdout().execute(LeaveAlternateScreen);
        let _ = stdout().execute(PopKeyboardEnhancementFlags);

        hook(info)
    });

    enable_raw_mode().unwrap();
    stdout()
        .execute(EnterAlternateScreen)
        .unwrap()
        .execute(PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
        ))
        .unwrap();

    loop {
        if event::poll(Duration::from_millis(20)).unwrap() {
            let event = event::read().unwrap();
            let exit = engine.event(event).unwrap();
            if exit {
                break;
            }
        }

        terminal.draw(|frame| engine.draw(frame)).unwrap();
    }

    let _ = disable_raw_mode();
    let _ = stdout().execute(LeaveAlternateScreen);
    let _ = stdout().execute(PopKeyboardEnhancementFlags);
}
