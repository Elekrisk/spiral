use std::{collections::HashMap, fmt::Display};

use log::debug;
use ratatui::crossterm::event::{KeyCode, KeyModifiers, MediaKeyCode};

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

impl Display for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let prefixes = self
            .modifiers
            .iter()
            .filter_map(|keymod| {
                Some(match keymod {
                    KeyModifiers::ALT => "A-",
                    KeyModifiers::CONTROL => "C-",
                    KeyModifiers::HYPER => "H-",
                    KeyModifiers::META => "M-",
                    KeyModifiers::NONE => return None,
                    KeyModifiers::SHIFT => "S-",
                    KeyModifiers::SUPER => "Su-",
                    _ => unreachable!(),
                })
            })
            .collect::<String>();
        let key = match self.code {
            KeyCode::Backspace => "bspc".to_string(),
            KeyCode::Enter => "enter".to_string(),
            KeyCode::Left => "left".to_string(),
            KeyCode::Right => "right".to_string(),
            KeyCode::Up => "up".to_string(),
            KeyCode::Down => "down".to_string(),
            KeyCode::Home => "home".to_string(),
            KeyCode::End => "end".to_string(),
            KeyCode::PageUp => "pageup".to_string(),
            KeyCode::PageDown => "pagedown".to_string(),
            KeyCode::Tab => "tab".to_string(),
            KeyCode::BackTab => "backtab".to_string(),
            KeyCode::Delete => "delete".to_string(),
            KeyCode::Insert => "insert".to_string(),
            KeyCode::F(n) => format!("f{n}"),
            KeyCode::Char(c) => match c {
                ' ' => "spc".to_string(),
                o => o.to_string(),
            },
            KeyCode::Null => "null".to_string(),
            KeyCode::Esc => "esc".to_string(),
            KeyCode::CapsLock => "caps".to_string(),
            KeyCode::ScrollLock => "scrolllock".to_string(),
            KeyCode::NumLock => "numlock".to_string(),
            KeyCode::PrintScreen => "printscreen".to_string(),
            KeyCode::Pause => "pause".to_string(),
            KeyCode::Menu => "menu".to_string(),
            KeyCode::KeypadBegin => "keypadbegin".to_string(),
            KeyCode::Media(key) => match key {
                MediaKeyCode::Play => "play".to_string(),
                MediaKeyCode::Pause => "pause".to_string(),
                MediaKeyCode::PlayPause => "playpause".to_string(),
                MediaKeyCode::Reverse => "reverse".to_string(),
                MediaKeyCode::Stop => "stop".to_string(),
                MediaKeyCode::FastForward => "fastforward".to_string(),
                MediaKeyCode::Rewind => "rewind".to_string(),
                MediaKeyCode::TrackNext => "tracknext".to_string(),
                MediaKeyCode::TrackPrevious => "trackprev".to_string(),
                MediaKeyCode::Record => "record".to_string(),
                MediaKeyCode::LowerVolume => "lowervolume".to_string(),
                MediaKeyCode::RaiseVolume => "raisevolume".to_string(),
                MediaKeyCode::MuteVolume => "mutevolume".to_string(),
            },
            KeyCode::Modifier(_) => unreachable!(),
        };

        write!(f, "{prefixes}{key}")
    }
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
