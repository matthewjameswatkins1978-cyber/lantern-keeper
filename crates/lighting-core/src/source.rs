//! The [`Source`] is the authoritative original material for Lantern Keeper.
//!
//! A Source preserves the exact bytes/characters supplied by the caller. It is
//! intentionally not normalised, trimmed, or rewritten. A
//! [`SourceFingerprint`] is a deterministic SHA-256 hash of the original
//! content; it identifies exact duplicates, not semantic similarity.

use std::fmt;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

/// Strongly typed Source identifier.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceId(String);

impl SourceId {
    fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    /// Creates a Source ID from a UUID string.
    ///
    /// # Errors
    ///
    /// Returns `None` if the supplied string is not a valid UUID.
    pub fn parse(value: &str) -> Option<Self> {
        uuid::Uuid::parse_str(value)
            .ok()
            .map(|_| Self(value.to_owned()))
    }

    /// Reconstitutes a Source ID from a raw record identifier string.
    ///
    /// Repository implementations are responsible for extracting the key
    /// portion from their persistence layer (e.g. the `id` of a SurrealDB
    /// `Thing`) before calling this constructor.
    pub fn from_record_id(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    /// Returns the identifier as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// The kind of original material a Source represents.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceKind {
    /// Unformatted plain text.
    PlainText,
    /// Markdown text.
    Markdown,
}

/// Non-empty title for a Source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceTitle(String);

impl SourceTitle {
    /// Creates a title, rejecting empty or whitespace-only values.
    pub fn new(value: impl Into<String>) -> Result<Self, SourceError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(SourceError::EmptyTitle);
        }
        Ok(Self(value))
    }

    /// Returns the title as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Original Source content.
///
/// `Display` is deliberately not implemented and `Debug` redacts the body so
/// that Source text cannot leak into ordinary logs by accident.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceContent(String);

impl SourceContent {
    /// Creates content, rejecting empty values.
    ///
    /// Whitespace is preserved exactly.
    pub fn new(value: impl Into<String>) -> Result<Self, SourceError> {
        let value = value.into();
        if value.is_empty() {
            return Err(SourceError::EmptyContent);
        }
        Ok(Self(value))
    }

    /// Returns the original content as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the original content bytes.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl fmt::Debug for SourceContent {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SourceContent")
            .field("len", &self.0.len())
            .finish()
    }
}

/// Deterministic fingerprint of exact Source content.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SourceFingerprint(String);

impl SourceFingerprint {
    /// Computes a fingerprint from the exact content bytes.
    pub fn for_content(content: &SourceContent) -> Self {
        let hash = Sha256::digest(content.as_bytes());
        Self(hex(&hash))
    }

    /// Returns the fingerprint as a lowercase hex string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Errors that can occur when constructing a Source.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SourceError {
    #[error("source title cannot be empty or whitespace only")]
    EmptyTitle,
    #[error("source content cannot be empty")]
    EmptyContent,
}

/// Input for creating a new [`Source`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewSource {
    pub kind: SourceKind,
    pub title: SourceTitle,
    pub content: SourceContent,
}

/// Authoritative original material.
///
/// The content is preserved exactly as supplied. The fingerprint is computed
/// from the original bytes, so identical content always yields the same
/// fingerprint regardless of title or kind.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Source {
    id: SourceId,
    kind: SourceKind,
    title: SourceTitle,
    content: SourceContent,
    fingerprint: SourceFingerprint,
    created_at: DateTime<Utc>,
    previous_version_id: Option<SourceId>,
}

/// Result of attempting to store a Source through a repository.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreSourceResult {
    /// The Source was stored for the first time.
    Stored(Source),
    /// An identical Source (by fingerprint) already exists.
    Duplicate {
        /// The ID of the existing Source with the same content.
        existing_id: SourceId,
        /// The attempted Source.
        attempted: Source,
    },
}

/// Repository boundary for persisting and retrieving Sources.
///
/// Implementations live outside `lighting-core` so the domain never depends on
/// a concrete storage engine.
#[async_trait]
pub trait SourceRepository: Send + Sync {
    /// Stores a Source if its content fingerprint is not already present.
    async fn store(&self, source: Source) -> Result<StoreSourceResult, SourceRepositoryError>;

    /// Retrieves a Source by ID, returning `None` if not found.
    async fn get(&self, id: &SourceId) -> Result<Option<Source>, SourceRepositoryError>;

    /// Resolves the current revision of a logical Source identified by (kind, title).
    ///
    /// When a revision chain exists (A → B → C via `previous_version_id`), the
    /// source whose ID is not referenced by any other `previous_version_id` in
    /// the same logical group is the current revision.
    async fn get_current(
        &self,
        kind: SourceKind,
        title: &SourceTitle,
    ) -> Result<Option<Source>, SourceRepositoryError>;
}

/// Failure categories returned by a Source repository.
#[derive(Debug, Error)]
pub enum SourceRepositoryError {
    #[error("failed to connect to the source store")]
    Connection(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("source store operation failed")]
    Operation(#[source] Box<dyn std::error::Error + Send + Sync>),
}

impl Source {
    /// Creates a new Source from the supplied input, generating an ID,
    /// fingerprint, and UTC timestamp.
    pub fn create(input: NewSource) -> Self {
        let fingerprint = SourceFingerprint::for_content(&input.content);
        Self::reconstitute(
            SourceId::new(),
            input.kind,
            input.title,
            input.content,
            fingerprint,
            Utc::now(),
            None,
        )
    }

    /// Reconstitutes a Source from persisted fields.
    ///
    /// This is intended for repository implementations that need to rebuild the
    /// domain object without generating a new ID or timestamp.
    pub fn reconstitute(
        id: SourceId,
        kind: SourceKind,
        title: SourceTitle,
        content: SourceContent,
        fingerprint: SourceFingerprint,
        created_at: DateTime<Utc>,
        previous_version_id: Option<SourceId>,
    ) -> Self {
        Self {
            id,
            kind,
            title,
            content,
            fingerprint,
            created_at,
            previous_version_id,
        }
    }

    /// Returns the Source identifier.
    pub fn id(&self) -> &SourceId {
        &self.id
    }

    /// Returns the Source kind.
    pub fn kind(&self) -> SourceKind {
        self.kind
    }

    /// Returns the Source title.
    pub fn title(&self) -> &SourceTitle {
        &self.title
    }

    /// Returns the original Source content.
    pub fn content(&self) -> &SourceContent {
        &self.content
    }

    /// Returns the deterministic content fingerprint.
    pub fn fingerprint(&self) -> &SourceFingerprint {
        &self.fingerprint
    }

    /// Returns the creation timestamp.
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Returns the previous version ID, if this is a revision.
    pub fn previous_version_id(&self) -> Option<&SourceId> {
        self.previous_version_id.as_ref()
    }
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(out, "{byte:02x}");
    }
    out
}

/// Finds all byte-offsets of `needle` in `haystack`.
///
/// Returns `Vec<usize>` with each match's starting byte position.
/// This is the shared exact-text rebasing primitive used by both
/// project-scoped and marker-led retrieval.
pub fn find_all_matches(haystack: &str, needle: &str) -> Vec<usize> {
    if needle.is_empty() {
        return vec![];
    }
    haystack
        .as_bytes()
        .windows(needle.len())
        .enumerate()
        .filter(|(_, window)| *window == needle.as_bytes())
        .map(|(pos, _)| pos)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plaintext(title: &str, content: &str) -> Source {
        Source::create(NewSource {
            kind: SourceKind::PlainText,
            title: SourceTitle::new(title).unwrap(),
            content: SourceContent::new(content).unwrap(),
        })
    }

    fn markdown(title: &str, content: &str) -> Source {
        Source::create(NewSource {
            kind: SourceKind::Markdown,
            title: SourceTitle::new(title).unwrap(),
            content: SourceContent::new(content).unwrap(),
        })
    }

    #[test]
    fn creates_valid_plain_text_source() {
        let source = plaintext("Note", "Hello, world!");

        assert_eq!(source.kind(), SourceKind::PlainText);
        assert_eq!(source.title().as_str(), "Note");
        assert_eq!(source.content().as_str(), "Hello, world!");
        assert!(!source.fingerprint().as_str().is_empty());
    }

    #[test]
    fn creates_valid_markdown_source() {
        let source = markdown("Doc", "# Heading\n\nBody text.");

        assert_eq!(source.kind(), SourceKind::Markdown);
        assert_eq!(source.title().as_str(), "Doc");
        assert_eq!(source.content().as_str(), "# Heading\n\nBody text.");
        assert!(!source.fingerprint().as_str().is_empty());
    }

    #[test]
    fn rejects_empty_title() {
        assert_eq!(SourceTitle::new(""), Err(SourceError::EmptyTitle));
    }

    #[test]
    fn rejects_whitespace_only_title() {
        assert_eq!(SourceTitle::new("   "), Err(SourceError::EmptyTitle));
        assert_eq!(SourceTitle::new("\t\n"), Err(SourceError::EmptyTitle));
    }

    #[test]
    fn rejects_empty_content() {
        assert_eq!(SourceContent::new(""), Err(SourceError::EmptyContent));
    }

    #[test]
    fn preserves_content_exactly() {
        let content = "  leading\r\n\n\ntrailing  \t";
        let source = plaintext("Exact", content);

        assert_eq!(source.content().as_str(), content);
        assert_eq!(source.content().as_bytes(), content.as_bytes());
    }

    #[test]
    fn fingerprint_is_stable_for_identical_content() {
        let a = plaintext("A", "same content");
        let b = plaintext("B", "same content");

        assert_eq!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn same_content_with_different_titles_has_same_fingerprint() {
        let a = plaintext("First title", "duplicate body");
        let b = markdown("Second title", "duplicate body");

        assert_eq!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn changed_content_has_different_fingerprint() {
        let a = plaintext("A", "content one");
        let b = plaintext("A", "content two");

        assert_ne!(a.fingerprint(), b.fingerprint());
    }

    #[test]
    fn debug_does_not_leak_content() {
        let secret = "xyzzy-secret-marker-42";
        let source = plaintext("Sensitive", secret);

        let debug = format!("{source:?}");

        assert!(
            !debug.contains(secret),
            "Debug output must not contain source content, got: {debug}"
        );
    }
}
