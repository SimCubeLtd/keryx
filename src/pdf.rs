//! Deterministic PDF rendering for immutable Keryx draft versions.
//!
//! This module intentionally accepts HTML only from the server's blob store.
//! The HTTP and CLI surfaces deal in draft IDs and version numbers instead of
//! exposing Fulgur as a general-purpose HTML conversion service.

use std::sync::OnceLock;

use anyhow::{bail, Context, Result};
use base64::Engine as _;
use chrono::{DateTime, Datelike};
use fulgur::asset::AssetBundle;
use fulgur::config::{Margin, PageSize};
use fulgur::engine::Engine;
use fulgur::pagination_layout::implied_page_count;
use fulgur::paragraph::LineItem;
use scraper::node::Node;
use scraper::Html;

const MAX_PDF_PAGES: u32 = 200;
const MAX_PDF_BYTES: usize = 64 * 1024 * 1024;

const PRINT_CSS: &str = r#"
html, body {
  width: auto !important;
  min-width: 0 !important;
  max-width: none !important;
  overflow: visible !important;
  font-family: "Noto Sans", sans-serif !important;
}
body { margin: 0 !important; }
*, *::before, *::after { box-sizing: border-box !important; }
body > *, header > *, main, footer, [class~="shell"] {
  width: 100% !important;
  min-width: 0 !important;
  max-width: 100% !important;
  margin-left: 0 !important;
  margin-right: 0 !important;
}
[class~="shell"] {
  width: auto !important;
  max-width: none !important;
  margin-left: 6mm !important;
  margin-right: 6mm !important;
}
header {
  padding: 10mm 0 8mm !important;
  break-after: page;
  break-after: page !important;
}
nav { display: none !important; }
main { padding-bottom: 0 !important; }
section {
  padding: 10mm 0 !important;
  break-before: page;
  break-before: page !important;
}
h1 {
  max-width: 100% !important;
  font-size: 34pt !important;
  line-height: 1.02 !important;
}
h2 { font-size: 24pt !important; }
h3 { font-size: 13pt !important; }
h1, h2, h3, h4 {
  break-after: avoid;
  break-after: avoid !important;
}
h3, h4 {
  /* Fulgur 0.40 drops break-after: avoid. Reserve a few body lines so its
     fragmenter moves an orphaned subheading, then reclaim the excess gap. */
  min-height: 32mm !important;
  margin-bottom: -22mm !important;
}
p, li, dt, dd, th, td, code, pre {
  min-width: 0 !important;
  max-width: 100% !important;
  overflow-wrap: anywhere !important;
  word-break: break-word !important;
}
code, pre, kbd, samp { font-family: "Noto Sans", sans-serif !important; }
pre {
  white-space: pre-wrap !important;
  overflow: visible !important;
  break-inside: auto !important;
}
img, svg {
  max-width: 100% !important;
  height: auto !important;
}
svg {
  width: 100% !important;
  min-width: 0 !important;
}
.meta { grid-template-columns: repeat(2, minmax(0, 1fr)) !important; }
.grid, .grid.three, .verdict, .flow, .phase, .section-head {
  display: block !important;
}
.keryx-print-stack, [data-keryx-print="stack"] { display: block !important; }
.grid > *, .verdict > *, .flow > *, .phase > *, .section-head > * {
  width: 100% !important;
  min-width: 0 !important;
  max-width: 100% !important;
}
.keryx-print-stack > *, [data-keryx-print="stack"] > * {
  width: 100% !important;
  min-width: 0 !important;
  max-width: 100% !important;
}
.flow > * { min-height: 0 !important; }
.flow > *::after { display: none !important; }
.section-head > * + *, .phase > * + * { margin-top: 4mm !important; }
.contract { overflow: visible !important; }
.verdict, .cell, .callout, .phase, figure, [data-keryx-figure] {
  break-inside: avoid;
  break-inside: avoid !important;
}
table {
  /* A4 minus 15mm page margins and the 6mm inner gutter on each side. */
  width: 168mm !important;
  max-width: 168mm !important;
  table-layout: fixed !important;
  break-inside: auto !important;
  overflow: hidden !important;
}
thead { display: table-header-group; }
th, td { white-space: normal !important; }
tr {
  break-inside: avoid;
  break-inside: avoid !important;
}
.keryx-page-break {
  break-before: page;
  break-before: page !important;
}
.keryx-page-flow {
  break-before: auto;
  break-before: auto !important;
}
[data-keryx-page="break"] { break-before: page !important; }
[data-keryx-page="flow"] { break-before: auto !important; }
footer { padding: 8mm 0 0 !important; }
"#;

#[derive(Debug, Clone, Copy)]
pub struct PdfIdentity<'a> {
    pub draft_id: &'a str,
    pub version_number: i64,
    pub version_created_at: &'a str,
}

#[derive(Debug)]
pub struct RenderedPdf {
    pub bytes: Vec<u8>,
    pub page_count: u32,
    pub image_count: usize,
    pub svg_count: usize,
}

#[derive(Debug)]
struct EmbeddedImage {
    source: String,
    bytes: Vec<u8>,
}

/// Validate the print-only contract, stamp the selected Keryx identity into a
/// render copy, and return PDF bytes without touching the filesystem.
pub fn render_version_pdf(html: &str, identity: PdfIdentity<'_>) -> Result<RenderedPdf> {
    validate_identity(identity)?;
    let embedded_images = validate_pdf_html(html)?;
    let normalized_html = outline_inline_svg_text(html)?;

    let title = document_title(html).unwrap_or_else(|| "Untitled Keryx report".to_string());
    let materialized = materialize_versioned_html(&normalized_html, identity, &title)?;
    let engine = Engine::builder()
        .page_size(PageSize::A4)
        .margin(Margin::uniform_mm(15.0))
        .title(title)
        .creator("Keryx")
        .producer(format!("Keryx {} / Fulgur 0.40", env!("CARGO_PKG_VERSION")))
        .bookmarks(true)
        .tagged(true)
        .system_fonts(false)
        .assets(pdf_assets(&embedded_images))
        .build();

    let layout = engine
        .layout(&materialized)
        .context("laying out the Keryx PDF")?;
    let page_count = implied_page_count(&layout.geometry);
    if page_count > MAX_PDF_PAGES {
        bail!("PDF would contain {page_count} pages; maximum is {MAX_PDF_PAGES}");
    }

    let inline_image_count = layout
        .drawables
        .paragraphs
        .values()
        .flat_map(|paragraph| &paragraph.lines)
        .flat_map(|line| &line.items)
        .filter(|item| matches!(item, LineItem::Image(_)))
        .count();
    let image_count = layout.drawables.images.len() + inline_image_count;
    let svg_count = layout.drawables.svgs.len();
    let bytes = engine
        .render(&materialized)
        .context("rendering the Keryx PDF")?;
    if !bytes.starts_with(b"%PDF-") {
        bail!("renderer returned an invalid PDF");
    }
    if bytes.len() > MAX_PDF_BYTES {
        bail!(
            "PDF is {} bytes; maximum is {MAX_PDF_BYTES} bytes",
            bytes.len()
        );
    }

    Ok(RenderedPdf {
        bytes,
        page_count,
        image_count,
        svg_count,
    })
}

/// Add identity metadata, visible report metadata, print hooks, and repeating
/// footer content. The stored HTML is never modified.
pub fn materialize_versioned_html(
    html: &str,
    identity: PdfIdentity<'_>,
    title: &str,
) -> Result<String> {
    let publication_date = publication_date(identity.version_created_at)?;
    let escaped_title = crate::render::escape_html(title);
    let head = format!(
        r#"<meta name="keryx-draft-id" content="{}">
<meta name="keryx-version" content="{}">
<style data-keryx-pdf>
@page {{
  size: A4;
  margin: 15mm 15mm 18mm;
  @bottom-left {{ content: "Date: {}"; font-size: 8pt; font-family: "Noto Sans"; color: #666; }}
  @bottom-right {{ content: "Page " counter(page) " of " counter(pages); font-size: 8pt; font-family: "Noto Sans"; color: #666; }}
}}
{}
[data-keryx-version]:empty::after {{ content: "{}"; }}
.keryx-pdf-header {{ width: 168mm !important; min-width: 0 !important; max-width: 168mm !important; margin: 0 6mm 8mm !important; padding: 2mm 0; border-bottom: .25mm solid #777; color: #555; font: 9pt/1.35 "Noto Sans"; overflow-wrap: anywhere !important; word-break: break-word !important; }}
</style>"#,
        identity.draft_id,
        identity.version_number,
        publication_date,
        PRINT_CSS,
        identity.version_number
    );
    let publication_header = format!(
        r#"<aside class="keryx-pdf-header" data-keryx-pdf-header>Title: {escaped_title} · Version: {}</aside>"#,
        identity.version_number
    );

    if find_opening_tag_start(html, "html").is_none()
        && find_opening_tag_start(html, "body").is_none()
        && find_opening_tag_start(html, "head").is_none()
    {
        let (doctype, content) = split_doctype(html);
        return Ok(format!(
            "{doctype}<html><head>{head}</head><body>{publication_header}{content}</body></html>"
        ));
    }

    let with_head = insert_before_closing_tag(html, "head", &head).unwrap_or_else(|| {
        if let Some(body_start) = find_opening_tag_start(html, "body") {
            let mut output = String::with_capacity(html.len() + head.len() + 14);
            output.push_str(&html[..body_start]);
            output.push_str("<head>");
            output.push_str(&head);
            output.push_str("</head>");
            output.push_str(&html[body_start..]);
            output
        } else {
            format!("<head>{head}</head>{html}")
        }
    });

    if let Some(body_start) = find_opening_tag_end(&with_head, "body") {
        let mut output = String::with_capacity(with_head.len() + publication_header.len());
        output.push_str(&with_head[..body_start]);
        output.push_str(&publication_header);
        output.push_str(&with_head[body_start..]);
        Ok(output)
    } else if let Some(head_end) = find_closing_tag_end(&with_head, "head") {
        let body_open = format!("<body>{publication_header}");
        let with_body = insert_at(&with_head, head_end, &body_open);
        Ok(insert_before_closing_tag(&with_body, "html", "</body>")
            .unwrap_or_else(|| format!("{with_body}</body>")))
    } else {
        Ok(format!("<body>{publication_header}{with_head}</body>"))
    }
}

fn publication_date(created_at: &str) -> Result<String> {
    let date = DateTime::parse_from_rfc3339(created_at)
        .with_context(|| format!("invalid Keryx version creation time {created_at:?}"))?;
    Ok(format!(
        "{} {} {}",
        date.day(),
        date.format("%B"),
        date.year()
    ))
}

fn validate_identity(identity: PdfIdentity<'_>) -> Result<()> {
    if identity.version_number < 1 {
        bail!("version must be at least 1");
    }
    if identity.draft_id.len() != 12
        || !identity
            .draft_id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    {
        bail!("invalid Keryx draft ID");
    }
    Ok(())
}

fn validate_pdf_html(html: &str) -> Result<Vec<EmbeddedImage>> {
    let document = Html::parse_document(html);
    let mut errors = Vec::new();
    let mut images: Vec<EmbeddedImage> = Vec::new();

    for node in document.tree.nodes() {
        let Node::Element(element) = node.value() else {
            continue;
        };
        let tag = element.name().to_ascii_lowercase();

        match tag.as_str() {
            "script" => push_unique(
                &mut errors,
                "PDF export does not support script-generated content.".into(),
            ),
            "canvas" => push_unique(
                &mut errors,
                "PDF export does not support <canvas>; use inline SVG instead.".into(),
            ),
            "style" => {
                let css = node
                    .descendants()
                    .filter_map(|descendant| match descendant.value() {
                        Node::Text(text) => Some(text.text.as_ref()),
                        _ => None,
                    })
                    .collect::<String>();
                validate_css_assets(&css, &mut errors);
            }
            _ => {}
        }

        for (name, value) in element.attrs() {
            if name.eq_ignore_ascii_case("style") {
                validate_css_assets(value, &mut errors);
            }
            let image_source = (tag == "img" && name.eq_ignore_ascii_case("src"))
                || (tag == "image"
                    && (name.eq_ignore_ascii_case("href")
                        || name.eq_ignore_ascii_case("xlink:href")));
            if image_source {
                match decode_embedded_image(value) {
                    Ok(bytes) => {
                        if !images.iter().any(|image| image.source == value) {
                            images.push(EmbeddedImage {
                                source: value.to_string(),
                                bytes,
                            });
                        }
                    }
                    Err(()) => push_unique(
                        &mut errors,
                        format!(
                            "PDF export requires base64-embedded PNG, JPEG, or GIF images; unsupported source on <{tag}>."
                        ),
                    ),
                }
            }
        }
    }

    if errors.is_empty() {
        Ok(images)
    } else {
        bail!("HTML is not PDF-compatible:\n- {}", errors.join("\n- "))
    }
}

fn validate_css_assets(css: &str, errors: &mut Vec<String>) {
    let normalized: String = css
        .chars()
        .filter(|character| !character.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect();
    if normalized.contains("@import") {
        push_unique(
            errors,
            "PDF export does not support CSS @import rules.".into(),
        );
    }

    let mut remaining = normalized.as_str();
    while let Some(start) = remaining.find("url(") {
        let after = &remaining[start + 4..];
        let Some(end) = after.find(')') else {
            break;
        };
        let value = after[..end].trim_matches(['\'', '"']);
        if !value.starts_with('#') {
            push_unique(
                errors,
                "PDF export does not support CSS image URLs; use an embedded <img> or inline SVG."
                    .into(),
            );
        }
        remaining = &after[end + 1..];
    }
}

fn decode_embedded_image(value: &str) -> std::result::Result<Vec<u8>, ()> {
    let (metadata, encoded) = value.trim().split_once(',').ok_or(())?;
    let metadata = metadata.to_ascii_lowercase();
    if !metadata.ends_with(";base64") {
        return Err(());
    }
    let mime = metadata
        .strip_prefix("data:")
        .and_then(|value| value.strip_suffix(";base64"))
        .ok_or(())?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(
            encoded
                .chars()
                .filter(|character| !character.is_whitespace())
                .collect::<String>(),
        )
        .map_err(|_| ())?;
    let valid_magic = match mime {
        "image/png" => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" | "image/jpg" => bytes.starts_with(&[0xff, 0xd8, 0xff]),
        "image/gif" => bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a"),
        _ => false,
    };
    valid_magic.then_some(bytes).ok_or(())
}

fn document_title(html: &str) -> Option<String> {
    let document = Html::parse_document(html);
    document.tree.nodes().find_map(|node| match node.value() {
        Node::Element(element) if element.name().eq_ignore_ascii_case("title") => {
            let title = node
                .descendants()
                .filter_map(|descendant| match descendant.value() {
                    Node::Text(text) => Some(text.text.as_ref()),
                    _ => None,
                })
                .collect::<String>();
            let title = title.trim();
            (!title.is_empty()).then(|| title.to_string())
        }
        _ => None,
    })
}

/// Convert inline SVG text to vector paths using the bundled font. Blitz's
/// SVG parser uses host fonts independently of Fulgur's font bundle, which can
/// otherwise make labels disappear when system fonts are disabled.
fn outline_inline_svg_text(html: &str) -> Result<String> {
    let lowercase = html.to_ascii_lowercase();
    let mut output = String::with_capacity(html.len());
    let mut cursor = 0;

    while let Some(relative_start) = find_svg_tag(&lowercase[cursor..], false) {
        let start = cursor + relative_start;
        let end = find_svg_end(&lowercase, start)
            .with_context(|| format!("inline SVG at byte {start} has no closing </svg> tag"))?;
        let svg = &html[start..end];

        output.push_str(&html[cursor..start]);
        if find_ascii_case_insensitive(svg, "<text").is_some() {
            output.push_str(&outline_svg_text(svg).context("outlining inline SVG text")?);
        } else {
            output.push_str(svg);
        }
        cursor = end;
    }

    output.push_str(&html[cursor..]);
    Ok(output)
}

fn outline_svg_text(svg: &str) -> Result<String> {
    let mut options = usvg::Options {
        font_family: "Noto Sans".to_string(),
        ..usvg::Options::default()
    };
    let fontdb = options.fontdb_mut();
    fontdb.load_font_data(svg_font_data().clone());
    fontdb.set_serif_family("Noto Sans");
    fontdb.set_sans_serif_family("Noto Sans");
    fontdb.set_monospace_family("Noto Sans");

    let tree = usvg::Tree::from_str(svg, &options).context("parsing inline SVG")?;
    Ok(tree.to_string(&usvg::WriteOptions::default()))
}

fn svg_font_data() -> &'static Vec<u8> {
    static FONT: OnceLock<Vec<u8>> = OnceLock::new();
    FONT.get_or_init(|| {
        let encoded = include_str!("../assets/NotoSans-Regular.woff2.b64");
        let compressed = base64::engine::general_purpose::STANDARD
            .decode(encoded.split_whitespace().collect::<String>())
            .expect("bundled Noto Sans base64 must be valid");
        let mut input = compressed.as_slice();
        woff2_patched::decode::convert_woff2_to_ttf(&mut input)
            .expect("bundled Noto Sans WOFF2 must decode")
    })
}

fn find_svg_end(lowercase_html: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut cursor = start;

    loop {
        let open = find_svg_tag(&lowercase_html[cursor..], false).map(|index| cursor + index);
        let close = find_svg_tag(&lowercase_html[cursor..], true).map(|index| cursor + index);

        match (open, close) {
            (Some(open), Some(close)) if open < close => {
                depth += 1;
                cursor = open + 4;
            }
            (_, Some(close)) => {
                if depth == 0 {
                    return None;
                }
                depth -= 1;
                let tag_end = lowercase_html[close..].find('>')? + close + 1;
                if depth == 0 {
                    return Some(tag_end);
                }
                cursor = tag_end;
            }
            _ => return None,
        }
    }
}

fn find_svg_tag(html: &str, closing: bool) -> Option<usize> {
    let needle = if closing { "</svg" } else { "<svg" };
    let mut cursor = 0;
    while let Some(index) = html[cursor..].find(needle) {
        let index = cursor + index;
        let boundary = html.as_bytes().get(index + needle.len()).copied();
        if boundary.is_some_and(|byte| byte.is_ascii_whitespace() || byte == b'>') {
            return Some(index);
        }
        cursor = index + needle.len();
    }
    None
}

fn pdf_assets(embedded_images: &[EmbeddedImage]) -> AssetBundle {
    static ASSETS: OnceLock<AssetBundle> = OnceLock::new();
    let mut assets = ASSETS
        .get_or_init(|| {
            let encoded = include_str!("../assets/NotoSans-Regular.woff2.b64");
            let font = base64::engine::general_purpose::STANDARD
                .decode(encoded.split_whitespace().collect::<String>())
                .expect("bundled Noto Sans base64 must be valid");
            let mut assets = AssetBundle::new();
            assets
                .add_font_bytes(font)
                .expect("bundled Noto Sans must be a valid WOFF2 font");
            assets
        })
        .clone();
    for image in embedded_images {
        assets.add_image(image.source.clone(), image.bytes.clone());
    }
    assets
}

fn insert_before_closing_tag(html: &str, tag: &str, value: &str) -> Option<String> {
    let needle = format!("</{tag}>");
    let index = find_ascii_case_insensitive(html, &needle)?;
    let mut output = String::with_capacity(html.len() + value.len());
    output.push_str(&html[..index]);
    output.push_str(value);
    output.push_str(&html[index..]);
    Some(output)
}

fn find_opening_tag_end(html: &str, tag: &str) -> Option<usize> {
    let start = find_opening_tag_start(html, tag)?;
    html[start..].find('>').map(|offset| start + offset + 1)
}

fn find_opening_tag_start(html: &str, tag: &str) -> Option<usize> {
    find_ascii_case_insensitive(html, &format!("<{tag}"))
}

fn find_closing_tag_end(html: &str, tag: &str) -> Option<usize> {
    let closing = format!("</{tag}>");
    find_ascii_case_insensitive(html, &closing).map(|start| start + closing.len())
}

fn insert_at(html: &str, index: usize, value: &str) -> String {
    let mut output = String::with_capacity(html.len() + value.len());
    output.push_str(&html[..index]);
    output.push_str(value);
    output.push_str(&html[index..]);
    output
}

fn split_doctype(html: &str) -> (&str, &str) {
    let trimmed = html.trim_start();
    if !trimmed
        .get(..9)
        .is_some_and(|prefix| prefix.eq_ignore_ascii_case("<!doctype"))
    {
        return ("", html);
    }
    let Some(end) = trimmed.find('>') else {
        return ("", html);
    };
    let leading = html.len() - trimmed.len();
    html.split_at(leading + end + 1)
}

fn find_ascii_case_insensitive(haystack: &str, needle: &str) -> Option<usize> {
    haystack
        .as_bytes()
        .windows(needle.len())
        .position(|window| window.eq_ignore_ascii_case(needle.as_bytes()))
}

fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.contains(&value) {
        values.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DRAFT_ID: &str = "abc123def456";
    const VERSION_CREATED_AT: &str = "2026-08-24T20:00:00.000Z";
    const PIXEL_GIF: &str =
        "data:image/gif;base64,R0lGODlhAQABAIAAAP///wAAACH5BAEAAAAALAAAAAABAAEAAAICRAEAOw==";

    fn identity(version_number: i64) -> PdfIdentity<'static> {
        PdfIdentity {
            draft_id: DRAFT_ID,
            version_number,
            version_created_at: VERSION_CREATED_AT,
        }
    }

    #[test]
    fn materializes_version_without_changing_the_source() {
        let source = "<!doctype html><html><head><title>Plan</title></head><body><span data-keryx-version></span><h1>Hello</h1></body></html>";
        let rendered = materialize_versioned_html(source, identity(7), "Plan").unwrap();

        assert!(rendered.contains("name=\"keryx-draft-id\" content=\"abc123def456\""));
        assert!(rendered.contains("name=\"keryx-version\" content=\"7\""));
        assert!(rendered.contains("Title: Plan · Version: 7"));
        assert!(rendered.contains("Date: 24 August 2026"));
        assert!(!rendered.contains("Keryx draft abc123def456"));
        assert!(rendered.contains("[data-keryx-version]:empty::after { content: \"7\"; }"));
        assert!(rendered.contains("table-layout: fixed !important"));
        assert!(rendered.contains("width: 168mm !important"));
        assert!(rendered.contains("margin-left: 6mm !important"));
        assert!(rendered.contains("overflow: hidden !important"));
        assert!(rendered.contains("break-after: page;"));
        assert!(rendered.contains("min-height: 32mm !important"));
        assert!(rendered.contains("margin-bottom: -22mm !important"));
        assert!(!source.contains("keryx-pdf-header"));
    }

    #[test]
    fn wraps_an_html_fragment_without_moving_the_doctype_into_the_body() {
        let rendered = materialize_versioned_html(
            "<!doctype html><title>Plan</title><h1>Hello</h1>",
            identity(2),
            "Plan",
        )
        .unwrap();

        assert!(rendered.starts_with("<!doctype html><html><head>"));
        assert!(rendered.contains("</head><body><aside"));
        assert!(rendered.ends_with("</body></html>"));
    }

    #[test]
    fn outlines_inline_svg_text_with_the_bundled_font() {
        let source = r##"<svg viewBox="0 0 240 80" width="100%">
<style>.label { fill: #ffffff; font: 18px sans-serif; }</style>
<rect width="240" height="80" fill="#111111"/>
<text class="label" x="16" y="48">Diagram label</text>
</svg>"##;

        let outlined = outline_inline_svg_text(source).unwrap();

        assert!(outlined.contains("<path"));
        assert!(!outlined.contains("<text"));
        assert!(!outlined.contains("Diagram label"));
    }

    #[test]
    fn leaves_text_free_inline_svg_unchanged() {
        let source = r#"<svg viewBox="0 0 10 10"><rect width="10" height="10"/></svg>"#;

        assert_eq!(outline_inline_svg_text(source).unwrap(), source);
    }

    #[test]
    fn rejects_dynamic_and_external_assets() {
        let error = render_version_pdf(
            "<title>x</title><script>draw()</script><canvas></canvas><img src=\"https://example.com/x.png\">",
            identity(1),
        )
        .unwrap_err()
        .to_string();

        assert!(error.contains("script-generated"));
        assert!(error.contains("<canvas>"));
        assert!(error.contains("embedded PNG"));
    }

    #[test]
    fn paginates_the_document_header_and_top_level_sections() {
        let html = r#"<!doctype html><html><head><title>Pages</title></head><body>
<header><h1>Cover</h1></header>
<main>
  <section><h2>First section</h2><p>First page of content.</p></section>
  <section><h2>Second section</h2><p>Second page of content.</p></section>
</main>
</body></html>"#;
        let rendered = render_version_pdf(html, identity(4)).unwrap();

        assert_eq!(rendered.page_count, 3);
    }

    #[test]
    fn page_flow_class_keeps_a_section_with_the_previous_section() {
        let html = r#"<!doctype html><html><head><title>Flow</title></head><body>
<header><h1>Cover</h1></header>
<main>
  <section><h2>First section</h2><p>First block.</p></section>
  <section class="keryx-page-flow"><h2>Second section</h2><p>Second block.</p></section>
</main>
</body></html>"#;
        let rendered = render_version_pdf(html, identity(5)).unwrap();

        assert_eq!(rendered.page_count, 2);
    }

    #[test]
    fn renders_representative_report_with_pages_image_and_vector_svg() {
        let html = format!(
            r##"<!doctype html>
<html><head><title>Representative Keryx report</title><style>
body {{ color: #111; background: #fff; }}
section {{ min-height: 210mm; }}
table {{ width: 100%; border-collapse: collapse; }}
td, th {{ border: 1px solid #777; padding: 4px; }}
</style></head><body>
<section><h1>Overview</h1><p>Long-form report content with <strong>semantic structure</strong>.</p>
<pre><code>fn main() {{ println!("keryx"); }}</code></pre>
<table><thead><tr><th>Signal</th><th>Result</th></tr></thead><tbody><tr><td>PDF</td><td>Ready</td></tr></tbody></table>
<img alt="embedded pixel" src="{PIXEL_GIF}" style="display:block;width:16px;height:16px">
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 320 120" role="img" aria-label="bar chart">
<rect x="20" y="60" width="60" height="40" fill="#9b87f5"></rect>
<rect x="120" y="35" width="60" height="65" fill="#22c55e"></rect>
<rect x="220" y="10" width="60" height="90" fill="#f59e0b"></rect>
</svg></section>
<section data-keryx-page="break"><h2>Decision</h2><p>Second section.</p></section>
</body></html>"##
        );
        let rendered = render_version_pdf(&html, identity(3)).unwrap();

        assert!(rendered.bytes.starts_with(b"%PDF-"));
        assert!(rendered.bytes.len() > 10_000);
        assert!(rendered.page_count >= 2);
        assert_eq!(rendered.image_count, 1);
        assert_eq!(rendered.svg_count, 1);
    }
}
