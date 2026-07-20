//! Markdown source outline parser.
//!
//! Parses ATX headings (1-6 `#` followed by whitespace) from Markdown content,
//! respecting fenced code blocks so that heading-looking lines inside code
//! blocks are ignored. Supports CRLF line endings, UTF-8, and optional closing
//! `#` markers.

use serde::Serialize;

/// A single heading extracted from a Markdown document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Heading {
    pub level: u8,
    pub title: String,
    pub start_byte: usize,
    pub end_byte: usize,
}

/// The structured outline of a Markdown Source.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct MarkdownOutline {
    pub headings: Vec<Heading>,
}

pub fn parse_outline(content: &str) -> MarkdownOutline {
    let headings_raw = extract_headings(content);
    let headings = compute_ranges(content, headings_raw);
    MarkdownOutline { headings }
}

struct RawHeading {
    level: u8,
    title: String,
    start_byte: usize,
}

fn extract_headings(content: &str) -> Vec<RawHeading> {
    let mut headings = Vec::new();
    let mut in_code_block = false;
    let mut fence_char: Option<char> = None;
    let mut fence_len: usize = 0;
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut pos = 0;

    while pos < len {
        let line_start = pos;
        let mut line_end = pos;
        while line_end < len && bytes[line_end] != b'\n' {
            line_end += 1;
        }

        let line_content =
            if line_end > line_start && line_end > 0 && bytes.get(line_end - 1) == Some(&b'\r') {
                &bytes[line_start..line_end - 1]
            } else {
                &bytes[line_start..line_end]
            };

        let line_str = std::str::from_utf8(line_content).unwrap_or("");

        let trimmed = line_str.trim_start();
        if let Some((fc, fl)) = fence_info(trimmed) {
            if !in_code_block {
                in_code_block = true;
                fence_char = Some(fc);
                fence_len = fl;
            } else if Some(fc) == fence_char && fl >= fence_len {
                in_code_block = false;
                fence_char = None;
                fence_len = 0;
            }
            pos = if line_end < len { line_end + 1 } else { len };
            continue;
        }

        if !in_code_block {
            if let Some(raw) = try_parse_heading_line(line_str, line_start) {
                headings.push(RawHeading {
                    level: raw.level,
                    title: raw.title,
                    start_byte: raw.start_byte,
                });
            }
        }

        pos = if line_end < len { line_end + 1 } else { len };
    }

    headings
}

fn try_parse_heading_line(line: &str, line_start_byte: usize) -> Option<RawHeading> {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut hash_count = 0u8;
    while i < bytes.len() && bytes[i] == b'#' && hash_count < 6 {
        hash_count += 1;
        i += 1;
    }
    if hash_count == 0 || hash_count > 6 {
        return None;
    }
    if i >= bytes.len() || (bytes[i] != b' ' && bytes[i] != b'\t') {
        return None;
    }
    let title = clean_heading_title(&line[i..]);
    Some(RawHeading {
        level: hash_count,
        title,
        start_byte: line_start_byte,
    })
}

fn clean_heading_title(raw: &str) -> String {
    let trimmed = raw.trim_start();
    let bytes = trimmed.as_bytes();
    let mut end = bytes.len();
    let mut hash_start = end;
    while hash_start > 0 && bytes[hash_start - 1] == b'#' {
        hash_start -= 1;
    }
    if hash_start < end
        && hash_start > 0
        && (bytes[hash_start - 1] == b' ' || bytes[hash_start - 1] == b'\t')
    {
        end = hash_start;
        while end > 0 && (bytes[end - 1] == b' ' || bytes[end - 1] == b'\t') {
            end -= 1;
        }
    }
    trimmed[..end].to_owned()
}

fn fence_info(trimmed: &str) -> Option<(char, usize)> {
    let bytes = trimmed.as_bytes();
    if bytes.len() < 3 {
        return None;
    }
    let ch = bytes[0];
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let mut count = 0usize;
    for &b in bytes.iter() {
        if b == ch {
            count += 1;
        } else {
            break;
        }
    }
    if count >= 3 {
        let rest = &bytes[count..];
        if rest.iter().all(|b| b.is_ascii_whitespace())
            || rest.first().map_or(true, |b| b.is_ascii_whitespace())
        {
            return Some((ch as char, count));
        }
        return Some((ch as char, count));
    }
    None
}

fn compute_ranges(content: &str, headings: Vec<RawHeading>) -> Vec<Heading> {
    let content_len = content.as_bytes().len();
    let mut result = Vec::with_capacity(headings.len());
    for (i, raw) in headings.iter().enumerate() {
        let end = find_section_end(&headings, i, content_len);
        result.push(Heading {
            level: raw.level,
            title: raw.title.clone(),
            start_byte: raw.start_byte,
            end_byte: end,
        });
    }
    result
}

fn find_section_end(headings: &[RawHeading], current_idx: usize, content_len: usize) -> usize {
    let current_level = headings[current_idx].level;
    for next in headings.iter().skip(current_idx + 1) {
        if next.level <= current_level {
            return next.start_byte;
        }
    }
    content_len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_document_returns_no_headings() {
        assert!(parse_outline("").headings.is_empty());
    }

    #[test]
    fn document_without_headings_returns_empty() {
        assert!(parse_outline("Just some text\nno headings here\n")
            .headings
            .is_empty());
    }

    #[test]
    fn single_h1_heading() {
        let outline = parse_outline("# Hello\n\nBody text.\n");
        assert_eq!(outline.headings.len(), 1);
        assert_eq!(outline.headings[0].level, 1);
        assert_eq!(outline.headings[0].title, "Hello");
        assert_eq!(outline.headings[0].start_byte, 0);
    }

    #[test]
    fn headings_level_1_through_6() {
        let outline =
            parse_outline("# One\n## Two\n### Three\n#### Four\n##### Five\n###### Six\n");
        assert_eq!(outline.headings.len(), 6);
    }

    #[test]
    fn heading_with_closing_hashes() {
        let outline = parse_outline("# Hello world #####\n\ncontent\n");
        assert_eq!(outline.headings[0].title, "Hello world");
    }

    #[test]
    fn heading_without_whitespace_after_hash_is_not_heading() {
        assert!(parse_outline("#not-a-heading\n").headings.is_empty());
    }

    #[test]
    fn heading_inside_backtick_fence_is_ignored() {
        let outline = parse_outline("```\n# Not a heading\n```\n\n# Real heading\n");
        assert_eq!(outline.headings.len(), 1);
        assert_eq!(outline.headings[0].title, "Real heading");
    }

    #[test]
    fn heading_inside_tilde_fence_is_ignored() {
        let outline = parse_outline("~~~\n# Not a heading\n~~~\n\n# Real heading\n");
        assert_eq!(outline.headings.len(), 1);
        assert_eq!(outline.headings[0].title, "Real heading");
    }

    #[test]
    fn unclosed_fence_hides_rest_of_headings() {
        assert!(parse_outline("```\n# Hidden\n").headings.is_empty());
    }

    #[test]
    fn sibling_headings_partition_content() {
        let content = "# A\n\ntext a\n\n# B\n\ntext b\n";
        let outline = parse_outline(content);
        assert_eq!(outline.headings.len(), 2);
        assert!(outline.headings[0].end_byte <= outline.headings[1].start_byte);
    }

    #[test]
    fn nested_headings_contain_children() {
        let outline =
            parse_outline("# Top\n\n## Child\n\nchild text\n\n# Sibling\n\nsibling text\n");
        assert_eq!(outline.headings.len(), 3);
    }

    #[test]
    fn crlf_headings_parse_correctly() {
        let outline = parse_outline("# Hello\r\n\r\nBody\r\n");
        assert_eq!(outline.headings.len(), 1);
        assert_eq!(outline.headings[0].title, "Hello");
    }

    #[test]
    fn heading_ranges_preserve_byte_offsets() {
        let content = "# Hello\n\nBody\n";
        let outline = parse_outline(content);
        assert_eq!(content.as_bytes()[outline.headings[0].start_byte], b'#');
        assert_eq!(outline.headings[0].end_byte, content.len());
    }
}
