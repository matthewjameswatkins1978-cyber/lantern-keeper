use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum IdentifierError {
    #[error("identifier cannot be empty")]
    Empty,
}

macro_rules! identifier_type {
    ($name:ident) => {
        #[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Result<Self, IdentifierError> {
                let value = value.into();
                if value.trim().is_empty() {
                    return Err(IdentifierError::Empty);
                }

                Ok(Self(value))
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl FromStr for $name {
            type Err = IdentifierError;

            fn from_str(value: &str) -> Result<Self, Self::Err> {
                Self::new(value)
            }
        }
    };
}

identifier_type!(ConversationId);
identifier_type!(EpisodeId);
identifier_type!(MarkerId);
identifier_type!(ProjectId);
identifier_type!(TopicId);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_reject_empty_values() {
        assert_eq!(ConversationId::new(""), Err(IdentifierError::Empty));
        assert_eq!(EpisodeId::new("   "), Err(IdentifierError::Empty));
        assert_eq!(MarkerId::new("\t"), Err(IdentifierError::Empty));
        assert_eq!(ProjectId::new("\n"), Err(IdentifierError::Empty));
        assert_eq!(TopicId::new(""), Err(IdentifierError::Empty));
    }

    #[test]
    fn identifiers_display_original_values() {
        let id = ConversationId::new("conversation-1").unwrap();

        assert_eq!(id.as_str(), "conversation-1");
        assert_eq!(id.to_string(), "conversation-1");
    }
}
