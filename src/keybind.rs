use std::collections::HashMap;

use log::debug;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};

use crate::{command::Command, mode::Mode};

pub struct Keybindings {
    pub binds: HashMap<Mode, HashMap<Key, Binding>>,
}

impl Keybindings {
    pub fn get(&self, mode: &Mode, seq: &[Key]) -> Option<&Binding> {
        let mut map = self.binds.get(mode)?;
        let mut binding = None;
        for key in seq {
            let mut b = map.get(key);
            if b.is_none()
                && key.modifiers.contains(KeyModifiers::SHIFT)
                && matches!(key.code, KeyCode::Char(_))
            {
                let key = Key {
                    code: key.code,
                    modifiers: key.modifiers.difference(KeyModifiers::SHIFT),
                };
                b = map.get(&key);
            }
            if let Some(b) = b {
                match b {
                    Binding::Group(g) => {
                        map = g;
                        binding = Some(b);
                    }
                    _ => {
                        binding = Some(b);
                    }
                }
            } else {
                debug!("No keybind for {key:?}");
                binding = None;
                break;
            }
        }

        binding
    }

    pub fn bind(&mut self, mode: &Mode, seq: &[Key], commands: Vec<String>) {
        if commands.is_empty() {
            panic!("Cannot bind a key to empty command list")
        }
        let pre = &seq[..seq.len() - 1];
        let key = seq[seq.len() - 1];
        let mut map = self.binds.entry(mode.clone()).or_default();
        for key in pre {
            if map.contains_key(key) {
                let b = map.get_mut(key).unwrap();
                match b {
                    Binding::Group(m) => {
                        map = m;
                    }
                    _ => {
                        panic!("Already bound");
                    }
                }
            } else {
                map.insert(*key, Binding::Group(HashMap::new()));
                map = map.get_mut(key).unwrap().as_group_mut().unwrap();
            }
        }
        map.insert(key, Binding::Commands(commands));
    }
}

pub enum Binding {
    Group(HashMap<Key, Binding>),
    Commands(Vec<String>),
}

impl Binding {
    pub fn as_group_mut(&mut self) -> Option<&mut HashMap<Key, Binding>> {
        if let Self::Group(v) = self {
            Some(v)
        } else {
            None
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Key {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

pub fn parse_key_sequence(seq: &str) -> anyhow::Result<Vec<Key>> {
    seq.split_whitespace()
        .map(|part| {
            let mut x = part.split('-').collect::<Vec<_>>();
            let key = x.pop().unwrap();
            let mut modifiers = KeyModifiers::NONE;
            for prefix in x {
                let modifier = match prefix {
                    "S" => KeyModifiers::SHIFT,
                    "C" => KeyModifiers::CONTROL,
                    "A" => KeyModifiers::ALT,
                    "Su" => KeyModifiers::SUPER,
                    "H" => KeyModifiers::HYPER,
                    "M" => KeyModifiers::META,
                    _ => anyhow::bail!("unrecognized key prefix"),
                };
                modifiers.insert(modifier);
            }

            let code = match key {
                "tab" => KeyCode::Tab,
                "backtab" => KeyCode::BackTab,
                "spc" => KeyCode::Char(' '),
                "bspc" => KeyCode::Backspace,
                "enter" => KeyCode::Enter,
                _ if key.chars().count() == 1 => KeyCode::Char(key.chars().next().unwrap()),
                _ => anyhow::bail!("unrecognized key {key}"),
            };

            Ok(Key { code, modifiers })
        })
        .collect()
}
