//! Response DTOs for the Source Outline API.

use chrono::{DateTime, Utc};
use serde::Serialize;

use lighting_core::{Heading, MarkdownOutline, Source, SourceKind};

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct SourceOutlineResponse {
    pub source_id: String,
    pub title: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub created_at: Option<DateTime<Utc>>,
    pub headings: Vec<HeadingDto>,
}

#[derive(Clone, Debug, Serialize, PartialEq, Eq)]
pub struct HeadingDto {
    pub level: u8,
    pub title: String,
    pub start_byte: usize,
    pub end_byte: usize,
}

impl From<&Heading> for HeadingDto {
    fn from(h: &Heading) -> Self {
        Self {
            level: h.level,
            title: h.title.clone(),
            start_byte: h.start_byte,
            end_byte: h.end_byte,
        }
    }
}

impl SourceOutlineResponse {
    pub fn from_source(source: &Source, outline: &MarkdownOutline) -> Self {
        let kind_str = match source.kind() {
            SourceKind::PlainText => "plain_text",
            SourceKind::Markdown => "markdown",
        };
        Self {
            source_id: source.id().as_str().to_owned(),
            title: source.title().as_str().to_owned(),
            kind: kind_str.to_owned(),
            created_at: Some(source.created_at()),
            headings: outline.headings.iter().map(HeadingDto::from).collect(),
        }
    }
}
