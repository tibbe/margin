//! Mermaid diagrams, drawn in the core so every editor shows the same
//! picture: merman lays them out as SVG, and resvg turns that into pixels.
//! See `docs/architecture.md`.

use merman::resources::{InputResourcePolicy, ResourceProfile};
use merman::svg::{
    RenderResourcePolicy, RootBackgroundPostprocessor, ScopedCssPostprocessor, SvgPipeline,
    SvgRenderOptions,
};
use merman::{
    Engine, MermaidConfig, OperationControl, ParseOptions, RenderError, RenderOutput,
    RenderRequest, Renderer, SvgEnvironment, SvgRequest,
};
use resvg::{tiny_skia, usvg};
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

/// How long a diagram may take to draw before it shows as an error.
const DEADLINE: Duration = Duration::from_secs(5);

/// The page's colors, as CSS hex, which a diagram is drawn in.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Colors {
    /// The page behind the diagram.
    pub background: String,
    /// Nodes: the code background.
    pub node: String,
    /// Around nodes.
    pub border: String,
    pub text: String,
    /// Lines and arrows: dimmed.
    pub line: String,
    /// The body font's size, in points: labels are drawn at it.
    pub font_size: u32,
}

/// A diagram drawn: its SVG, its size in points, and the labels it draws.
#[derive(Clone, Debug)]
pub struct Drawn {
    pub svg: String,
    pub width: f32,
    pub height: f32,
    pub labels: Vec<Label>,
}

/// Text a diagram draws, and where, in its own points.
#[derive(Clone, Debug, PartialEq)]
pub struct Label {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// The source doesn't parse.
    Syntax,
    /// Its first word names no diagram type merman knows: a typo, or a
    /// type newer than it.
    UnknownType,
    /// A type merman knows but can't draw.
    Unsupported,
    TooLarge,
    TooSlow,
    /// Anything else, a crash in the renderer included.
    Failed,
}

/// Why a diagram can't be drawn, said for the writer, and where in its
/// source, as a byte range, when that is known.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub failure: Failure,
    pub message: String,
    pub span: Option<Range<usize>>,
}

/// Draws a Mermaid diagram's source in `colors`.
pub fn render(source: &str, colors: &Colors) -> Result<Drawn, Error> {
    // merman is alpha: a panic in it is a diagram that can't be drawn, not
    // a crashed editor.
    let svg = match catch_unwind(AssertUnwindSafe(|| render_svg(source, colors))) {
        Ok(r) => r?,
        Err(_) => return Err(failed("Margin couldn’t draw this diagram.")),
    };
    let tree = parse(&svg).ok_or_else(|| failed("Margin couldn’t draw this diagram."))?;
    let size = tree.size();
    let mut labels = Vec::new();
    collect_labels(tree.root(), &mut labels);
    Ok(Drawn {
        svg,
        width: size.width(),
        height: size.height(),
        labels,
    })
}

/// A drawn diagram's SVG as `width`×`height` pixels: RGBA, premultiplied,
/// row by row.
pub fn rasterize(svg: &str, width: u32, height: u32) -> Option<Vec<u8>> {
    let tree = parse(svg)?;
    let mut pixmap = tiny_skia::Pixmap::new(width.max(1), height.max(1))?;
    let size = tree.size();
    let scale = tiny_skia::Transform::from_scale(
        pixmap.width() as f32 / size.width(),
        pixmap.height() as f32 / size.height(),
    );
    resvg::render(&tree, scale, &mut pixmap.as_mut());
    Some(pixmap.take())
}

fn render_svg(source: &str, colors: &Colors) -> Result<String, Error> {
    // Every color mermaid's base theme takes, by role: the page's, the
    // nodes', borders, text and lines.
    let mut vars = serde_json::Map::new();
    let mut set = |names: &[&str], value: &str| {
        for n in names {
            vars.insert((*n).to_string(), value.into());
        }
    };
    set(
        &[
            "background",
            "clusterBkg",
            "edgeLabelBackground",
            "sequenceNumberColor",
        ],
        &colors.background,
    );
    set(
        &[
            "primaryColor",
            "secondaryColor",
            "tertiaryColor",
            "mainBkg",
            "actorBkg",
            "labelBoxBkgColor",
            "noteBkgColor",
            "activationBkgColor",
        ],
        &colors.node,
    );
    set(
        &[
            "primaryBorderColor",
            "secondaryBorderColor",
            "tertiaryBorderColor",
            "nodeBorder",
            "clusterBorder",
            "actorBorder",
            "labelBoxBorderColor",
            "noteBorderColor",
            "activationBorderColor",
        ],
        &colors.border,
    );
    set(
        &[
            "primaryTextColor",
            "secondaryTextColor",
            "tertiaryTextColor",
            "textColor",
            "titleColor",
            "actorTextColor",
            "signalTextColor",
            "labelTextColor",
            "loopTextColor",
            "noteTextColor",
        ],
        &colors.text,
    );
    set(
        &["lineColor", "actorLineColor", "signalColor"],
        &colors.line,
    );
    set(
        &[
            "pieTitleTextColor",
            "pieSectionTextColor",
            "pieLegendTextColor",
        ],
        &colors.text,
    );
    set(&["pieStrokeColor"], &colors.background);
    set(&["pieOuterStrokeColor"], &colors.border);
    // Pie slices: shades from the nodes' color to the lines', neighbors far
    // apart, so each slice can be told apart.
    for (i, t) in [
        0.0, 0.5, 0.25, 0.75, 0.125, 0.625, 0.375, 0.875, 0.0625, 0.5625, 0.3125, 0.8125,
    ]
    .into_iter()
    .enumerate()
    {
        vars.insert(
            format!("pie{}", i + 1),
            mix(&colors.node, &colors.line, t).into(),
        );
    }
    vars.insert(
        "fontSize".to_string(),
        format!("{}px", colors.font_size).into(),
    );
    let config = MermaidConfig::from_value(serde_json::json!({
        "theme": "base",
        "fontFamily": "Helvetica Neue, Helvetica, Arial, sans-serif",
        "fontSize": colors.font_size,
        // Labels as SVG text, which resvg draws with their backgrounds.
        "htmlLabels": false,
        "flowchart": { "htmlLabels": false },
        // The look that takes its colors from the theme; mermaid 12's
        // default has colors of its own.
        "look": "classic",
        "themeVariables": vars,
    }));
    // The writer's own documents: the limits for trusted input, which draw
    // larger diagrams than those for pages from the web.
    let renderer = Renderer::new()
        .with_engine(Engine::new().with_site_config(config))
        .with_parse_options(ParseOptions::strict())
        .with_resource_policy(InputResourcePolicy::for_profile(
            ResourceProfile::TrustedNative,
        ));
    let request = SvgRequest {
        environment: SvgEnvironment::deterministic()
            .with_resource_policy(RenderResourcePolicy::trusted_native()),
        // No HTML labels: resvg draws no `foreignObject`. Edge labels sit on
        // the page's color, not half through it, so lines don't cross them.
        pipeline: Some(
            SvgPipeline::resvg_safe()
                .with_postprocessor(RootBackgroundPostprocessor::new(&colors.background))
                .with_postprocessor(ScopedCssPostprocessor::new(
                    ".edgeLabel rect { opacity: 1; }",
                )),
        ),
        options: SvgRenderOptions {
            diagram_id: Some("margin".to_string()),
            ..Default::default()
        },
        ..Default::default()
    };
    let control = OperationControl::new().with_deadline(DEADLINE);
    match renderer.render(RenderRequest::svg(source, control, request)) {
        Ok(RenderOutput::Svg(Some(svg))) => Ok(svg.svg().to_string()),
        Ok(_) | Err(RenderError::NoDiagram) => Err(unknown_type(source)),
        Err(e) => Err(error(source, &e)),
    }
}

fn error(source: &str, e: &RenderError) -> Error {
    match e {
        RenderError::Parse(d) => {
            let details = d.terminal_diagnostic_details();
            match details.code.as_str() {
                "merman.parse.no_diagram_detected" => unknown_type(source),
                "merman.parse.unsupported_diagram" => unsupported(details.diagram_type.as_deref()),
                _ => Error {
                    failure: Failure::Syntax,
                    message: parse_message(&d.to_string()),
                    span: details.span.map(|s| s.start..s.end.max(s.start)),
                },
            }
        }
        RenderError::ResourceLimitExceeded(_) => Error {
            failure: Failure::TooLarge,
            message: "This diagram is too large to draw.".to_string(),
            span: None,
        },
        RenderError::Cancelled(_) => Error {
            failure: Failure::TooSlow,
            message: "This diagram took too long to draw.".to_string(),
            span: None,
        },
        RenderError::Svg(merman::svg::RenderError::MissingCapability { diagram_type, .. })
        | RenderError::Svg(merman::svg::RenderError::UnsupportedDiagram { diagram_type }) => {
            unsupported(Some(diagram_type))
        }
        RenderError::Svg(merman::svg::RenderError::ResourceLimitExceeded(_)) => Error {
            failure: Failure::TooLarge,
            message: "This diagram is too large to draw.".to_string(),
            span: None,
        },
        RenderError::Svg(merman::svg::RenderError::Cancelled(_)) => Error {
            failure: Failure::TooSlow,
            message: "This diagram took too long to draw.".to_string(),
            span: None,
        },
        e => failed(&format!("Margin couldn’t draw this diagram: {e}")),
    }
}

/// The CSS hex color `t` of the way from `a` to `b`.
fn mix(a: &str, b: &str, t: f32) -> String {
    let rgb = |h: &str| -> [f32; 3] {
        let h = h.trim_start_matches('#');
        let c =
            |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0) as f32;
        [c(0), c(2), c(4)]
    };
    let (a, b) = (rgb(a), rgb(b));
    let c = |i: usize| (a[i] + (b[i] - a[i]) * t).round() as u8;
    format!("#{:02X}{:02X}{:02X}", c(0), c(1), c(2))
}

/// A parser's message without merman's "Diagram parse error (type): ".
fn parse_message(m: &str) -> String {
    let m = m.trim();
    match m
        .strip_prefix("Diagram parse error (")
        .and_then(|r| r.split_once("): "))
    {
        Some((_, rest)) => rest.to_string(),
        None => m.to_string(),
    }
}

/// The diagram's first word, which names its type: past front matter,
/// directives and comments.
fn type_word(source: &str) -> Option<&str> {
    let mut lines = source.lines().map(str::trim).peekable();
    if lines.peek() == Some(&"---") {
        lines.next();
        for l in lines.by_ref() {
            if l == "---" {
                break;
            }
        }
    }
    lines
        .find(|l| !l.is_empty() && !l.starts_with("%%"))
        .and_then(|l| l.split_whitespace().next())
}

fn unknown_type(source: &str) -> Error {
    Error {
        failure: Failure::UnknownType,
        message: match type_word(source) {
            Some(w) => format!("Unknown diagram type “{w}”."),
            None => "This diagram is empty.".to_string(),
        },
        span: None,
    }
}

fn unsupported(diagram_type: Option<&str>) -> Error {
    Error {
        failure: Failure::Unsupported,
        message: match diagram_type {
            Some(t) => format!("Margin can’t draw {t} diagrams yet."),
            None => "Margin can’t draw this kind of diagram yet.".to_string(),
        },
        span: None,
    }
}

fn failed(message: &str) -> Error {
    Error {
        failure: Failure::Failed,
        message: message.to_string(),
        span: None,
    }
}

/// The system's fonts, loaded once, which labels are drawn in.
fn fonts() -> Arc<usvg::fontdb::Database> {
    static FONTS: OnceLock<Arc<usvg::fontdb::Database>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let mut db = usvg::fontdb::Database::new();
            db.load_system_fonts();
            db.set_sans_serif_family("Helvetica Neue");
            Arc::new(db)
        })
        .clone()
}

fn parse(svg: &str) -> Option<usvg::Tree> {
    let options = usvg::Options {
        fontdb: fonts(),
        ..Default::default()
    };
    usvg::Tree::from_str(svg, &options).ok()
}

fn collect_labels(group: &usvg::Group, out: &mut Vec<Label>) {
    for node in group.children() {
        match node {
            usvg::Node::Group(g) => collect_labels(g, out),
            usvg::Node::Text(t) => {
                let text = t
                    .chunks()
                    .iter()
                    .map(|c| c.text().trim())
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" ");
                let b = t.abs_bounding_box();
                if !text.is_empty() {
                    out.push(Label {
                        text,
                        x: b.x(),
                        y: b.y(),
                        width: b.width(),
                        height: b.height(),
                    });
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn colors() -> Colors {
        Colors {
            background: "#ffffff".into(),
            node: "#f2f2f2".into(),
            border: "#d0d0d0".into(),
            text: "#1f1f1f".into(),
            line: "#6f6f6f".into(),
            font_size: 15,
        }
    }

    #[test]
    fn a_flowchart_draws_with_its_labels() {
        let d = render("flowchart TD\n  A[Start] --> B[Done]\n", &colors()).expect("drawn");
        assert!(
            d.width > 10.0 && d.height > 10.0,
            "{} × {}",
            d.width,
            d.height
        );
        let texts: Vec<&str> = d.labels.iter().map(|l| l.text.as_str()).collect();
        assert!(
            texts.contains(&"Start") && texts.contains(&"Done"),
            "{texts:?}"
        );
        for l in &d.labels {
            assert!(l.x >= 0.0 && l.x + l.width <= d.width + 1.0, "{l:?}");
        }
        let pixels = rasterize(&d.svg, 200, 100).expect("pixels");
        assert_eq!(pixels.len(), 200 * 100 * 4);
        assert!(pixels.chunks(4).any(|p| p[3] > 0), "something is drawn");
    }

    #[test]
    fn errors_say_what_is_wrong() {
        let syntax = render("flowchart TD\n  A[Start --> B\n", &colors()).unwrap_err();
        assert_eq!(syntax.failure, Failure::Syntax, "{syntax:?}");
        assert!(
            !syntax.message.starts_with("Diagram parse error"),
            "{}",
            syntax.message
        );
        let unknown = render("flowchat TD\n  A --> B\n", &colors()).unwrap_err();
        assert_eq!(unknown.failure, Failure::UnknownType);
        assert_eq!(unknown.message, "Unknown diagram type “flowchat”.");
        assert_eq!(
            render("", &colors()).unwrap_err().message,
            "This diagram is empty."
        );
        assert_eq!(
            type_word("---\ntitle: x\n---\n%% note\n\nsequenceDiagram\n"),
            Some("sequenceDiagram")
        );
    }

    #[test]
    fn colors_mix() {
        assert_eq!(mix("#000000", "#FFFFFF", 0.5), "#808080");
        assert_eq!(mix("#102030", "#102030", 0.3), "#102030");
    }

    #[test]
    fn parse_messages_lose_merman_prefix() {
        assert_eq!(
            parse_message("Diagram parse error (flowchart-v2): Unterminated node label"),
            "Unterminated node label"
        );
        assert_eq!(parse_message("Something else"), "Something else");
    }
}
