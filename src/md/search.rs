//! Finding text as the reader sees it: hidden syntax is skipped, so
//! searching for "bold text" finds `**bold** text`.

use super::doc::{Doc, LineKind};
use std::ops::Range;

/// The rendered text, one entry per visible character: the character and
/// its byte offset in the source. Line breaks inside paragraphs read as
/// spaces.
pub fn visible_chars(src: &str, doc: &Doc) -> Vec<(char, usize)> {
    let mut out = Vec::with_capacity(src.len());
    let mut soft = doc.soft_breaks.iter().peekable();
    for l in &doc.lines {
        if matches!(
            l.kind,
            LineKind::Blank | LineKind::SetextUnderline | LineKind::Fence | LineKind::Rule
        ) {
            continue;
        }
        for (off, c) in src[l.content_start..l.end].char_indices() {
            let p = l.content_start + off;
            if !doc.is_hidden_byte(p) {
                out.push((c, p));
            }
        }
        if l.end < src.len() {
            while soft.next_if(|&&s| s < l.end).is_some() {}
            let is_soft = soft.next_if(|&&s| s == l.end).is_some();
            out.push((if is_soft { ' ' } else { '\n' }, l.end));
        }
    }
    out
}

fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Every match of `needle` in the visible text, as source byte ranges.
pub fn find_all(src: &str, doc: &Doc, needle: &str, match_case: bool) -> Vec<Range<usize>> {
    let needle: Vec<char> = if match_case {
        needle.chars().collect()
    } else {
        needle.chars().map(fold).collect()
    };
    if needle.is_empty() {
        return Vec::new();
    }
    let hay = visible_chars(src, doc);
    let eq = |a: char, b: char| if match_case { a == b } else { fold(a) == b };
    let mut out = Vec::new();
    let mut i = 0;
    while i + needle.len() <= hay.len() {
        if needle.iter().enumerate().all(|(k, &n)| eq(hay[i + k].0, n)) {
            let (last, last_pos) = hay[i + needle.len() - 1];
            out.push(hay[i].1..last_pos + last.len_utf8());
            i += needle.len();
        } else {
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::super::doc::parse;
    use super::*;

    fn found<'a>(src: &'a str, needle: &str) -> Vec<&'a str> {
        let doc = parse(src);
        find_all(src, &doc, needle, false)
            .into_iter()
            .map(|r| &src[r])
            .collect()
    }

    #[test]
    fn matches_rendered_text_across_hidden_syntax() {
        assert_eq!(found("Some **bold** text\n", "bold text"), vec!["bold** text"]);
        assert_eq!(found("# Title\n", "title"), vec!["Title"]);
        assert_eq!(found("- item one\n", "- item"), Vec::<&str>::new());
        assert_eq!(found("[docs](https://x.y) here\n", "docs here"), vec!["docs](https://x.y) here"]);
    }

    #[test]
    fn soft_breaks_read_as_spaces() {
        assert_eq!(found("two\nweeks\n", "two weeks"), vec!["two\nweeks"]);
        assert!(found("# a\nb\n", "a b").is_empty());
    }

    #[test]
    fn case_folding_and_repeats() {
        assert_eq!(found("Aa aA\n", "aa").len(), 2);
        let src = "Aa aA\n";
        let doc = parse(src);
        assert_eq!(find_all(src, &doc, "aA", true).len(), 1);
    }
}
