#![feature(try_blocks)]

pub mod buffer;
pub mod view;
pub mod selection;
pub mod mode;
pub mod engine;
pub mod keybind;
pub mod command;
pub mod lua;

use std::{
    collections::HashMap,
    fs::File,
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
};

use buffer::{Buffer, BufferId};
use log::{debug, error, warn};
use ropey::Rope;
use view::{View, ViewId};