use std::{fmt::Display, str::FromStr};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Mode {
    Normal,
    Insert,
    Custom(String),
}

impl FromStr for Mode {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "normal" => Ok(Mode::Normal),
            "insert" => Ok(Mode::Insert),
            o => Ok(Mode::Custom(o.into())),
        }
    }
}

impl Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Mode::Normal => f.write_str("normal"),
            Mode::Insert => f.write_str("insert"),
            Mode::Custom(mode) => f.write_str(&mode.to_lowercase()),
        }
    }
}
