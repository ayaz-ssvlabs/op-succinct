use std::{
    fmt::{self, Display},
    str::FromStr,
};

use kona_proof::{errors::HintParsingError, HintType};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AltDAHintType {
    Standard(HintType),
    AltDACommitment,
}

impl FromStr for AltDAHintType {
    type Err = HintParsingError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "altda-commitment" => Ok(Self::AltDACommitment),
            _ => Ok(Self::Standard(HintType::from_str(value)?)),
        }
    }
}

impl From<AltDAHintType> for &str {
    fn from(value: AltDAHintType) -> Self {
        match value {
            AltDAHintType::AltDACommitment => "altda-commitment",
            AltDAHintType::Standard(hint_type) => hint_type.into(),
        }
    }
}

impl Display for AltDAHintType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s: &str = (*self).into();
        write!(f, "{s}")
    }
}
