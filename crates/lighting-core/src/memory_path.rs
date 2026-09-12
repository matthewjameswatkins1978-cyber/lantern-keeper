//! Memory path domain types: Marker → Episode → Project → exact Source range.
//!
//! Pure domain vocabulary. No storage, HTTP, or infrastructure dependencies.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::source::{SourceContent, SourceId};
use crate::{EpisodeId, MarkerId, ProjectId};

// ---------------------------------------------------------------------------
// SourceRange — an exact, safe byte range within one Source
// ---------------------------------------------------------------------------

/// An exact UTF-8 byte range within a single Source.
///
/// End is exclusive.  Constructed only via [`SourceRange::new`] which enforces
/// that the range fits the content and sits on valid UTF-8 boundaries.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRange {
    source_id: SourceId,
    start_byte: usize,
    end_byte: usize,
}

/// Errors that can occur when constructing a [`SourceRange`].
#[derive(Debug, Error, PartialEq, Eq)]
pub enum SourceRangeError {
    #[error("start_byte ({start}) must be strictly less than end_byte ({end})")]
    Inverted { start: usize, end: usize },
    #[error("end_byte ({end}) exceeds source content length ({len})")]
    OutOfBounds { end: usize, len: usize },
    #[error("start_byte ({start}) splits a multi-byte UTF-8 character")]
    InvalidUtf8Start { start: usize },
    #[error("end_byte ({end}) does not sit at a valid UTF-8 character boundary")]
    InvalidUtf8End { end: usize },
}

impl SourceRange {
    /// Creates a new `SourceRange` after verifying:
    ///
    /// * `start_byte < end_byte`
    /// * `end_byte <= content.len()`
    /// * Both boundaries fall on valid UTF-8 character boundaries.
    pub fn new(
        source_id: SourceId,
        start_byte: usize,
        end_byte: usize,
        content: &SourceContent,
    ) -> Result<Self, SourceRangeError> {
        if start_byte >= end_byte {
            return Err(SourceRangeError::Inverted {
                start: start_byte,
                end: end_byte,
            });
        }

        let bytes = content.as_bytes();
        if end_byte > bytes.len() {
            return Err(SourceRangeError::OutOfBounds {
                end: end_byte,
                len: bytes.len(),
            });
        }

        // Verify start sits on a valid UTF-8 char boundary
        if !is_char_boundary(bytes, start_byte) {
            return Err(SourceRangeError::InvalidUtf8Start { start: start_byte });
        }

        // Verify end sits on a valid UTF-8 char boundary
        if !is_char_boundary(bytes, end_byte) {
            return Err(SourceRangeError::InvalidUtf8End { end: end_byte });
        }

        Ok(Self {
            source_id,
            start_byte,
            end_byte,
        })
    }

    /// Returns the ID of the Source this range belongs to.
    pub fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    /// Inclusive start byte offset.
    pub fn start_byte(&self) -> usize {
        self.start_byte
    }

    /// Exclusive end byte offset.
    pub fn end_byte(&self) -> usize {
        self.end_byte
    }

    /// Reconstitutes a `SourceRange` from persisted fields without re-validating
    /// content boundaries.  Only for repository implementations that need to
    /// rebuild the domain object.  The range was already validated when the
    /// Episode was first created.
    ///
    /// # Safety
    ///
    /// Callers must guarantee the byte offsets are valid for the original Source
    /// content that was present at creation time.
    pub fn reconstitute(source_id: SourceId, start_byte: usize, end_byte: usize) -> Self {
        Self {
            source_id,
            start_byte,
            end_byte,
        }
    }

    /// Returns the exact UTF-8 slice from the given content.
    ///
    /// # Safety
    ///
    /// This is safe because construction guarantees the range is valid
    /// for the content (or for any content with identical bytes).
    pub fn slice<'a>(&self, content: &'a SourceContent) -> &'a str {
        let bytes = content.as_bytes();
        // This unwrap is safe because the constructor verified the
        // boundaries are on valid UTF-8 character boundaries.
        std::str::from_utf8(&bytes[self.start_byte..self.end_byte])
            .expect("SourceRange is on valid UTF-8 boundaries")
    }
}

/// Checks whether `pos` sits on a valid UTF-8 character boundary.
fn is_char_boundary(bytes: &[u8], pos: usize) -> bool {
    if pos == 0 || pos >= bytes.len() {
        return pos <= bytes.len();
    }
    // A valid char boundary is either the start of a UTF-8 sequence
    // (leading byte 0xxxxxxx or continuation byte 0xxxxxxx... actually
    // continuation bytes are 10xxxxxx — they are NOT boundaries).
    // The stdlib check is: `str::from_utf8(&bytes[..pos]).is_ok()`.
    std::str::from_utf8(&bytes[..pos]).is_ok()
}

// ---------------------------------------------------------------------------
// Project
// ---------------------------------------------------------------------------

/// Status of a Project.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectStatus {
    Active,
    Paused,
    Archived,
}

/// Non-blank project name.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ProjectName(String);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ProjectError {
    #[error("project name must not be blank")]
    BlankName,
}

impl ProjectName {
    pub fn new(value: impl Into<String>) -> Result<Self, ProjectError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(ProjectError::BlankName);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A minimal Project.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Project {
    id: ProjectId,
    name: ProjectName,
    status: ProjectStatus,
    created_at: DateTime<Utc>,
}

impl Project {
    pub fn new(name: ProjectName, status: ProjectStatus) -> Self {
        Self {
            id: ProjectId::new(uuid::Uuid::new_v4().to_string()).expect("UUID is never blank"),
            name,
            status,
            created_at: Utc::now(),
        }
    }

    pub fn id(&self) -> &ProjectId {
        &self.id
    }

    pub fn name(&self) -> &ProjectName {
        &self.name
    }

    pub fn status(&self) -> ProjectStatus {
        self.status
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

// ---------------------------------------------------------------------------
// Episode
// ---------------------------------------------------------------------------

/// Non-blank episode title.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EpisodeTitle(String);

#[derive(Debug, Error, PartialEq, Eq)]
pub enum EpisodeError {
    #[error("episode title must not be blank")]
    BlankTitle,
}

impl EpisodeTitle {
    pub fn new(value: impl Into<String>) -> Result<Self, EpisodeError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(EpisodeError::BlankTitle);
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A minimal Episode that points into a Source via a [`SourceRange`].
///
/// An Episode is not a copied summary and never outranks The Source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Episode {
    id: EpisodeId,
    title: EpisodeTitle,
    source_range: SourceRange,
    created_at: DateTime<Utc>,
}

impl Episode {
    pub fn new(title: EpisodeTitle, source_range: SourceRange) -> Self {
        Self {
            id: EpisodeId::new(uuid::Uuid::new_v4().to_string()).expect("UUID is never blank"),
            title,
            source_range,
            created_at: Utc::now(),
        }
    }

    pub fn id(&self) -> &EpisodeId {
        &self.id
    }

    pub fn title(&self) -> &EpisodeTitle {
        &self.title
    }

    pub fn source_range(&self) -> &SourceRange {
        &self.source_range
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

// ---------------------------------------------------------------------------
// Marker — a retrieval handle
// ---------------------------------------------------------------------------

/// A Marker is a retrieval handle, not evidence or truth.
///
/// The original display text is preserved exactly.  The lookup key normalises
/// whitespace and case so similar markers collide deterministically without
/// needing fuzzy matching in the hot path.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Marker {
    id: MarkerId,
    display_text: String,
    lookup_key: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum MarkerError {
    #[error("marker display text must not be blank")]
    BlankText,
}

impl Marker {
    /// Creates a new Marker, preserving the display text exactly.
    pub fn new(display_text: impl Into<String>) -> Result<Self, MarkerError> {
        let display_text = display_text.into();
        if display_text.trim().is_empty() {
            return Err(MarkerError::BlankText);
        }
        let lookup_key = normalise_lookup(&display_text);
        Ok(Self {
            id: MarkerId::new(uuid::Uuid::new_v4().to_string()).expect("UUID is never blank"),
            display_text,
            lookup_key,
            created_at: Utc::now(),
        })
    }

    pub fn id(&self) -> &MarkerId {
        &self.id
    }

    /// The exact display text as supplied.
    pub fn display_text(&self) -> &str {
        &self.display_text
    }

    /// Deterministic lookup key: trimmed, whitespace-collapsed, lowercased.
    pub fn lookup_key(&self) -> &str {
        &self.lookup_key
    }

    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }
}

/// Normalises text for Marker lookup: trim outer whitespace, collapse internal
/// whitespace to single spaces, lowercase.
fn normalise_lookup(text: &str) -> String {
    let trimmed = text.trim();
    let collapsed: String = trimmed.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.to_lowercase()
}

// ---------------------------------------------------------------------------
// Links
// ---------------------------------------------------------------------------

/// Strength / classification of an Episode → Project link.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectLinkKind {
    Primary,
    Secondary,
    Possible,
}

/// Links an Episode to a Project with a specific association strength.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeProjectLink {
    episode_id: EpisodeId,
    project_id: ProjectId,
    kind: ProjectLinkKind,
}

impl EpisodeProjectLink {
    pub fn new(episode_id: EpisodeId, project_id: ProjectId, kind: ProjectLinkKind) -> Self {
        Self {
            episode_id,
            project_id,
            kind,
        }
    }

    pub fn episode_id(&self) -> &EpisodeId {
        &self.episode_id
    }

    pub fn project_id(&self) -> &ProjectId {
        &self.project_id
    }

    pub fn kind(&self) -> ProjectLinkKind {
        self.kind
    }
}

/// Result of storing a Marker — either newly created or returned existing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreMarkerResult {
    Created(Marker),
    Existing(Marker),
}

/// Repository contract for persisting memory-path types.
///
/// Implementations live outside `lighting-core`.
#[async_trait::async_trait]
pub trait MemoryPathRepository: Send + Sync {
    /// Store a Project.
    async fn create_project(&self, project: Project) -> Result<Project, MemoryPathRepositoryError>;

    /// Retrieve a Project by ID.
    async fn get_project(
        &self,
        id: &ProjectId,
    ) -> Result<Option<Project>, MemoryPathRepositoryError>;

    /// Store an Episode. Must fail if its Source does not exist.
    async fn create_episode(&self, episode: Episode) -> Result<Episode, MemoryPathRepositoryError>;

    /// Retrieve an Episode by ID.
    async fn get_episode(
        &self,
        id: &EpisodeId,
    ) -> Result<Option<Episode>, MemoryPathRepositoryError>;

    /// Find an Episode with the same exact Source byte range.
    ///
    /// This default keeps existing in-memory adapters source-compatible while
    /// durable implementations can use it to make importer resume idempotent.
    async fn find_episode_by_source_range(
        &self,
        source_id: &SourceId,
        start_byte: usize,
        end_byte: usize,
    ) -> Result<Option<Episode>, MemoryPathRepositoryError> {
        let _ = (source_id, start_byte, end_byte);
        Ok(None)
    }

    /// Store a Marker. Returns existing Marker if lookup key collides.
    async fn create_marker(
        &self,
        marker: Marker,
    ) -> Result<StoreMarkerResult, MemoryPathRepositoryError>;

    /// Retrieve a Marker by ID.
    async fn get_marker(&self, id: &MarkerId) -> Result<Option<Marker>, MemoryPathRepositoryError>;

    /// Find a Marker by its normalized lookup key.
    async fn find_marker_by_lookup(
        &self,
        lookup_key: &str,
    ) -> Result<Option<Marker>, MemoryPathRepositoryError>;

    /// Link an Episode to a Project.
    async fn link_episode_project(
        &self,
        link: EpisodeProjectLink,
    ) -> Result<(), MemoryPathRepositoryError>;

    /// Link a Marker to an Episode.
    async fn link_episode_marker(
        &self,
        link: EpisodeMarkerLink,
    ) -> Result<(), MemoryPathRepositoryError>;

    /// List Project links for an Episode.
    async fn list_episode_project_links(
        &self,
        episode_id: &EpisodeId,
    ) -> Result<Vec<EpisodeProjectLink>, MemoryPathRepositoryError>;

    /// List Marker links for an Episode.
    async fn list_episode_marker_links(
        &self,
        episode_id: &EpisodeId,
    ) -> Result<Vec<EpisodeMarkerLink>, MemoryPathRepositoryError>;

    /// List Episodes linked to a Marker via native graph traversal.
    /// Results ordered by Episode created_at ascending, then episode_id ascending. Hard limit 50.
    async fn list_marker_episode_links(
        &self,
        marker_id: &MarkerId,
    ) -> Result<Vec<EpisodeMarkerLink>, MemoryPathRepositoryError>;

    /// List Episodes linked to a Project via native graph traversal.
    /// Results ordered by Episode created_at ascending, then episode_id ascending. Hard limit 50.
    async fn list_project_episode_links(
        &self,
        project_id: &ProjectId,
    ) -> Result<Vec<EpisodeProjectLink>, MemoryPathRepositoryError>;

    /// Find a Project-linked Episode by source and exact byte range.
    ///
    /// Used for writeback idempotency: before creating a new Episode for a
    /// recorded result, the service checks whether the Project already has a
    /// linked Episode that covers the same Source range. Returns the first
    /// matching Episode if found.
    async fn find_project_episode_by_source_range(
        &self,
        project_id: &ProjectId,
        source_id: &SourceId,
        start_byte: usize,
        end_byte: usize,
    ) -> Result<Option<Episode>, MemoryPathRepositoryError>;

    /// List all Projects.
    ///
    /// Ordered by created_at ascending, then project_id ascending.
    async fn list_all_projects(&self) -> Result<Vec<Project>, MemoryPathRepositoryError>;
}

/// Failure categories returned by a memory-path repository.
#[derive(Debug, thiserror::Error)]
pub enum MemoryPathRepositoryError {
    #[error("failed to connect to the store")]
    Connection(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("store operation failed")]
    Operation(#[source] Box<dyn std::error::Error + Send + Sync>),
    #[error("referenced Source does not exist")]
    MissingSource,
    #[error("record could not be decoded")]
    Decode,
}

/// Links a Marker to an Episode.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EpisodeMarkerLink {
    episode_id: EpisodeId,
    marker_id: MarkerId,
}

impl EpisodeMarkerLink {
    pub fn new(episode_id: EpisodeId, marker_id: MarkerId) -> Self {
        Self {
            episode_id,
            marker_id,
        }
    }

    pub fn episode_id(&self) -> &EpisodeId {
        &self.episode_id
    }

    pub fn marker_id(&self) -> &MarkerId {
        &self.marker_id
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::{SourceContent, SourceId};

    fn source_id() -> SourceId {
        SourceId::parse("a1b2c3d4-e5f6-7890-abcd-ef1234567890").expect("valid uuid")
    }

    fn content(s: &str) -> SourceContent {
        SourceContent::new(s).expect("non-empty content")
    }

    // ---- SourceRange ----

    #[test]
    fn valid_range_returns_exact_slice() {
        let c = content("Hello, world!");
        let range = SourceRange::new(source_id(), 0, 5, &c).expect("valid range");

        assert_eq!(range.start_byte(), 0);
        assert_eq!(range.end_byte(), 5);
        assert_eq!(range.slice(&c), "Hello");
    }

    #[test]
    fn range_must_have_start_less_than_end() {
        let c = content("abc");
        assert_eq!(
            SourceRange::new(source_id(), 0, 0, &c),
            Err(SourceRangeError::Inverted { start: 0, end: 0 })
        );
        assert_eq!(
            SourceRange::new(source_id(), 2, 1, &c),
            Err(SourceRangeError::Inverted { start: 2, end: 1 })
        );
    }

    #[test]
    fn range_exceeding_content_is_rejected() {
        let c = content("abc");
        assert_eq!(
            SourceRange::new(source_id(), 0, 5, &c),
            Err(SourceRangeError::OutOfBounds { end: 5, len: 3 })
        );
        assert_eq!(
            SourceRange::new(source_id(), 2, 10, &c),
            Err(SourceRangeError::OutOfBounds { end: 10, len: 3 })
        );
    }

    #[test]
    fn range_splitting_multibyte_utf8_is_rejected() {
        // 'é' = U+00E9 = 0xC3 0xA9 (two bytes)
        let c = content("café"); // bytes: c a f 0xC3 0xA9 = 5 bytes
        // start=0, end=3 splits between 'f' (byte 2) and first byte of 'é'
        // That IS valid — end=3 is after 'f', start of 'é'.
        // But start=3, end=4 is start at 0xC3 (continuation of 'é') — that should pass actually,
        // because start at the leading byte of a multi-byte char IS valid.
        // The invalid case is end=4 splitting the 'é': bytes 3,4 = 0xC3,0xA9. end=4 is after
        // the second byte, which is a continuation byte start — that's actually also valid.
        // The real invalid case: end in the MIDDLE of a multi-byte char. Let's try end=4 for 5-byte
        // content "café": bytes[4] = 0xA9, so start 3 end 4 should be "é" — valid.
        // Let's try start=3 end=4 — result is "é" — VALID.
        // The problem: start AT a continuation byte. Let's try start=4 on "café" — start=4
        // is byte 0xA9 which is a continuation byte — INVALID.
        assert_eq!(
            SourceRange::new(source_id(), 4, 5, &c),
            Err(SourceRangeError::InvalidUtf8Start { start: 4 })
        );
    }

    #[test]
    fn end_on_continuation_byte_is_rejected() {
        // 'é' = U+00E9 = 0xC3 0xA9
        let c = content("café");
        // end=4 puts end on 0xA9 which is a continuation byte — INVALID
        assert_eq!(
            SourceRange::new(source_id(), 0, 4, &c),
            Err(SourceRangeError::InvalidUtf8End { end: 4 })
        );
    }

    #[test]
    fn full_content_range_is_valid() {
        let c = content("Hello, 世界!");
        let range = SourceRange::new(source_id(), 0, c.as_bytes().len(), &c)
            .expect("full range should be valid");
        assert_eq!(range.slice(&c), "Hello, 世界!");
    }

    #[test]
    fn multibyte_start_and_end_are_valid() {
        let c = content("Hello, 世界!");
        // 'H'(0) 'e'(1) 'l'(2) 'l'(3) 'o'(4) ','(5) ' '(6)
        // '世'(7-9) '界'(10-12) '!'(13-13)
        let range =
            SourceRange::new(source_id(), 7, 14, &c).expect("multibyte range should be valid");
        assert_eq!(range.slice(&c), "世界!");
    }

    // ---- Project ----

    #[test]
    fn project_rejects_blank_name() {
        assert_eq!(ProjectName::new(""), Err(ProjectError::BlankName));
        assert_eq!(ProjectName::new("   "), Err(ProjectError::BlankName));
    }

    #[test]
    fn project_creates_with_valid_name() {
        let name = ProjectName::new("Lantern Keeper").expect("valid name");
        let project = Project::new(name, ProjectStatus::Active);
        assert_eq!(project.name().as_str(), "Lantern Keeper");
        assert_eq!(project.status(), ProjectStatus::Active);
        assert!(!project.id().as_str().is_empty());
    }

    // ---- Episode ----

    #[test]
    fn episode_rejects_blank_title() {
        assert_eq!(EpisodeTitle::new(""), Err(EpisodeError::BlankTitle));
        assert_eq!(EpisodeTitle::new("\t"), Err(EpisodeError::BlankTitle));
    }

    #[test]
    fn episode_creates_and_links_to_source_range() {
        let c = content("## Important Section\n\nSome text here.");
        // "## Important Section" = 20 bytes of ASCII
        let range = SourceRange::new(source_id(), 0, 20, &c).expect("valid range");
        let title = EpisodeTitle::new("Intro").expect("valid title");
        let episode = Episode::new(title, range.clone());

        assert_eq!(episode.title().as_str(), "Intro");
        assert_eq!(episode.source_range(), &range);
        assert_eq!(episode.source_range().slice(&c), "## Important Section");
    }

    // ---- Marker ----

    #[test]
    fn marker_rejects_blank_text() {
        assert_eq!(Marker::new(""), Err(MarkerError::BlankText));
        assert_eq!(Marker::new("   "), Err(MarkerError::BlankText));
    }

    #[test]
    fn marker_preserves_display_text() {
        let marker = Marker::new("  IMPORTANT — Setup  ").expect("valid text");
        assert_eq!(marker.display_text(), "  IMPORTANT — Setup  ");
    }

    #[test]
    fn marker_lookup_normalises_case_and_whitespace() {
        let a = Marker::new("  IMPORTANT — Setup  ").expect("valid");
        let b = Marker::new("important — setup").expect("valid");

        assert_eq!(a.display_text(), "  IMPORTANT — Setup  ");
        assert_eq!(b.display_text(), "important — setup");
        assert_eq!(a.lookup_key(), b.lookup_key());
        assert_eq!(a.lookup_key(), "important — setup");
    }

    #[test]
    fn marker_lookup_collapses_internal_whitespace() {
        let a = Marker::new("hello   world").expect("valid");
        let b = Marker::new("hello world").expect("valid");
        assert_eq!(a.lookup_key(), b.lookup_key());
        assert_eq!(a.lookup_key(), "hello world");
    }

    // ---- Links ----

    #[test]
    fn episode_project_link_preserves_ids_and_kind() {
        let eid = EpisodeId::new("ep-1").expect("valid");
        let pid = ProjectId::new("prj-1").expect("valid");
        let link = EpisodeProjectLink::new(eid.clone(), pid.clone(), ProjectLinkKind::Primary);

        assert_eq!(link.episode_id(), &eid);
        assert_eq!(link.project_id(), &pid);
        assert_eq!(link.kind(), ProjectLinkKind::Primary);
    }

    #[test]
    fn episode_marker_link_preserves_ids() {
        let eid = EpisodeId::new("ep-1").expect("valid");
        let mid = MarkerId::new("mk-1").expect("valid");
        let link = EpisodeMarkerLink::new(eid.clone(), mid.clone());

        assert_eq!(link.episode_id(), &eid);
        assert_eq!(link.marker_id(), &mid);
    }

    #[test]
    fn project_link_all_kinds() {
        let eid = EpisodeId::new("ep-x").expect("valid");
        let pid = ProjectId::new("prj-x").expect("valid");

        for kind in [
            ProjectLinkKind::Primary,
            ProjectLinkKind::Secondary,
            ProjectLinkKind::Possible,
        ] {
            let link = EpisodeProjectLink::new(eid.clone(), pid.clone(), kind);
            assert_eq!(link.kind(), kind);
        }
    }
}
