//! Finding text as the reader sees it: hidden syntax is skipped, so
//! searching for "bold text" finds `**bold** text`.

use super::doc::{Doc, LineKind};
use std::ops::Range;

/// The rendered text, one entry per visible character: the character and
/// its byte offset in the source. Line breaks inside paragraphs read as
/// spaces. Image blocks show their alt text, as in an editor that draws
/// none of them.
pub fn visible_chars(src: &str, doc: &Doc) -> Vec<(char, usize)> {
    shown_chars(src, doc, &|_| true, &|_| true)
}

/// [`visible_chars`], where image block `i` (in [`Doc::images`]) shows its
/// alt text when `shows_alt(i)`, else shows as the image, which reads as
/// nothing; and diagram `d` (in [`Doc::diagrams`]) shows its source when
/// `shows_source(d)`, else as the diagram, whose labels aren't text here.
fn shown_chars(
    src: &str,
    doc: &Doc,
    shows_alt: &dyn Fn(usize) -> bool,
    shows_source: &dyn Fn(usize) -> bool,
) -> Vec<(char, usize)> {
    let mut pictures: Vec<Range<usize>> = (0..doc.images.len())
        .filter(|&i| !shows_alt(i))
        .map(|i| doc.images[i].range.clone())
        .chain(
            (0..doc.diagrams.len())
                .filter(|&d| !shows_source(d))
                .map(|d| doc.diagrams[d].range.clone()),
        )
        .collect();
    pictures.sort_by_key(|r| r.start);
    let in_picture = |p: usize| {
        let i = pictures.partition_point(|r| r.end <= p);
        pictures.get(i).is_some_and(|r| r.start <= p)
    };
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
            if !doc.is_hidden_byte(p) && !in_picture(p) {
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

/// Every match of `needle` in the visible text, as source byte ranges,
/// with every image block showing its alt text (see [`visible_chars`]).
pub fn find_all(src: &str, doc: &Doc, needle: &str, match_case: bool) -> Vec<Range<usize>> {
    find_shown(src, doc, needle, match_case, |_| true, |_| true)
}

/// Every match of `needle` in the text as shown, as source byte ranges.
/// Image block `i` (in [`Doc::images`]) shows its alt text when
/// `shows_alt(i)`, as one that can't be loaded does; drawn as the image,
/// it matches nothing, neither its alt text nor its path. Diagram `d` (in
/// [`Doc::diagrams`]) shows its source when `shows_source(d)`, as one that
/// can't be drawn does; drawn, its source matches nothing.
pub fn find_shown(
    src: &str,
    doc: &Doc,
    needle: &str,
    match_case: bool,
    shows_alt: impl Fn(usize) -> bool,
    shows_source: impl Fn(usize) -> bool,
) -> Vec<Range<usize>> {
    let needle: Vec<char> = if match_case {
        needle.chars().collect()
    } else {
        needle.chars().map(fold).collect()
    };
    if needle.is_empty() {
        return Vec::new();
    }
    let hay = shown_chars(src, doc, &shows_alt, &shows_source);
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
        assert_eq!(
            found("Some **bold** text\n", "bold text"),
            vec!["bold** text"]
        );
        assert_eq!(found("# Title\n", "title"), vec!["Title"]);
        assert_eq!(found("- item one\n", "- item"), Vec::<&str>::new());
        assert_eq!(
            found("[docs](https://x.y) here\n", "docs here"),
            vec!["docs](https://x.y) here"]
        );
    }

    #[test]
    fn soft_breaks_read_as_spaces() {
        assert_eq!(found("two\nweeks\n", "two weeks"), vec!["two\nweeks"]);
        assert!(found("# a\nb\n", "a b").is_empty());
    }

    #[test]
    fn images_drawn_as_images_match_nothing() {
        let src = "Intro ![shot](a.png) here.\n\n![shot](b.png)\n\n![shot](c.png)\n";
        let doc = parse(src);
        let found = |shows_alt: &dyn Fn(usize) -> bool| -> Vec<usize> {
            find_shown(src, &doc, "shot", false, shows_alt, |_| true)
                .into_iter()
                .map(|r| r.start)
                .collect()
        };
        let starts: Vec<usize> = src.match_indices("shot").map(|(i, _)| i).collect();
        // Only the image in running text shows its alt text.
        assert_eq!(found(&|_| false), [starts[0]]);
        // A missing image's alt text shows, so it matches.
        assert_eq!(found(&|i| i == 1), [starts[0], starts[2]]);
        assert!(find_shown(src, &doc, "c.png", false, |_| false, |_| true).is_empty());
        // Showing every image as its alt text, as `find_all` does.
        assert_eq!(find_all(src, &doc, "shot", false).len(), 3);
    }

    #[test]
    fn diagrams_drawn_match_nothing_of_their_source() {
        let src = "Start here.\n\n```mermaid\nflowchart TD\n  A[Start] --> B\n```\n";
        let doc = parse(src);
        let n = |shows: bool| find_shown(src, &doc, "start", false, |_| true, |_| shows).len();
        assert_eq!(n(false), 1);
        assert_eq!(n(true), 2);
    }

    #[test]
    fn case_folding_and_repeats() {
        assert_eq!(found("Aa aA\n", "aa").len(), 2);
        let src = "Aa aA\n";
        let doc = parse(src);
        assert_eq!(find_all(src, &doc, "aA", true).len(), 1);
    }
}
