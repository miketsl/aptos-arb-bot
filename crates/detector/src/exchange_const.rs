use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Exchange {
    Tapp,
    Hyperion,
    Thala,
}

impl fmt::Display for Exchange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Exchange::Tapp => write!(f, "tapp"),
            Exchange::Hyperion => write!(f, "hyperion"),
            Exchange::Thala => write!(f, "thala"),
        }
    }
}

impl FromStr for Exchange {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "tapp" => Ok(Exchange::Tapp),
            "hyperion" => Ok(Exchange::Hyperion),
            "thala" => Ok(Exchange::Thala),
            _ => Err(()),
        }
    }
}
