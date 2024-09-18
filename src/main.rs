#![feature(try_blocks)]
#![feature(macro_metavar_expr)]
#![feature(iterator_try_collect)]
#![feature(panic_update_hook)]
#![feature(let_chains)]
#![feature(iter_intersperse)]
#![feature(get_many_mut)]

mod buffer;
mod command;
mod engine;
mod event;
mod keybind;
mod kill_ring;
mod lua;
mod mode;
mod selection;
mod view;

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
        self,
        event::{
            KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
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
    #[arg(long, short)]
    config: Option<PathBuf>,
    #[arg(long)]
    ignore_global_config: bool,
}

fn main() {
    env_logger::Builder::from_default_env()
        .filter_level(log::LevelFilter::Trace)
        .target(env_logger::Target::Pipe(Box::new(
            std::fs::File::create("./log.log").unwrap(),
        )))
        .init();

    let mut options = Options::parse();
    let path = options.path.take();

    let engine = Engine::new(options).unwrap();
    if let Err(e) = engine.reload_config() {
        eprintln!("{e}");
        return;
    }
    if let Some(path) = path {
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
    let _ = stdout()
        .execute(EnterAlternateScreen)
        .unwrap()
        .execute(PushKeyboardEnhancementFlags(
            KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES,
        ));

    loop {
        if crossterm::event::poll(Duration::from_millis(20)).unwrap() {
            let event = crossterm::event::read().unwrap();
            let exit = engine.event(event).unwrap();
            if exit {
                break;
            }
            engine.process_events().unwrap();
        }

        terminal.draw(|frame| engine.draw(frame)).unwrap();
    }

    let _ = disable_raw_mode();
    let _ = stdout().execute(LeaveAlternateScreen);
    let _ = stdout().execute(PopKeyboardEnhancementFlags);
}
