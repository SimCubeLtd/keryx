//! Upload-time HTML safety policy.
//!
//! Uploaded HTML may contain inline classic JavaScript. External script
//! sources, module scripts, inline event-handler attributes, unsafe URL
//! protocols, forms, iframes/embeds, `<link>`/`<base>`, srcdoc, and
//! meta-refresh redirects are rejected. Once stored, a draft is served
//! verbatim.

use scraper::node::Node;
use scraper::Html;
use url::Url;

pub const DEFAULT_MAX_HTML_BYTES: usize = 512 * 1024;

const BLOCKED_TAGS: &[&str] = &[
    "form", "iframe", "object", "embed", "applet", "base", "link",
];
const URL_ATTRS: &[&str] = &[
    "href",
    "src",
    "action",
    "formaction",
    "poster",
    "srcdoc",
    "xlink:href",
];
const BLOCKED_PROTOCOLS: &[&str] = &["javascript:", "vbscript:", "file:"];
const ALLOWED_SCRIPT_TYPES: &[&str] = &["", "text/javascript", "application/javascript"];

// Far above any real document (browsers themselves flatten around 512).
const MAX_DEPTH: usize = 512;

#[derive(Debug, Default)]
pub struct Validation {
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    pub title: Option<String>,
    pub has_inline_script: bool,
    pub external_image_hosts: Vec<String>,
}

impl Validation {
    pub fn ok(&self) -> bool {
        self.errors.is_empty()
    }
}

pub fn validate_html(html: &str, max_bytes: usize) -> Validation {
    let mut v = Validation::default();

    if html.trim().is_empty() {
        push_unique(&mut v.errors, "HTML document is empty.".into());
        return v;
    }

    let byte_length = html.len();
    if byte_length > max_bytes {
        push_unique(
            &mut v.errors,
            format!("HTML document is {byte_length} bytes; maximum is {max_bytes} bytes."),
        );
    }

    let document = Html::parse_document(html);
    let mut image_hosts: Vec<String> = Vec::new();
    let mut too_deep = false;

    let mut stack = vec![(document.tree.root(), 0usize)];
    while let Some((node, depth)) = stack.pop() {
        if let Node::Element(element) = node.value() {
            let tag = element.name().to_ascii_lowercase();

            if BLOCKED_TAGS.contains(&tag.as_str()) {
                push_unique(&mut v.errors, format!("Blocked <{tag}> tag found."));
            }

            if tag == "script" {
                v.has_inline_script = true;
                if element
                    .attrs()
                    .any(|(name, _)| name.eq_ignore_ascii_case("src"))
                {
                    push_unique(
                        &mut v.errors,
                        "External script sources are not allowed.".into(),
                    );
                }
                let script_type = element
                    .attrs()
                    .find(|(name, _)| name.eq_ignore_ascii_case("type"))
                    .map(|(_, value)| value.trim().to_ascii_lowercase())
                    .unwrap_or_default();
                if !ALLOWED_SCRIPT_TYPES.contains(&script_type.as_str()) {
                    push_unique(
                        &mut v.errors,
                        format!("Unsupported script type \"{script_type}\" found."),
                    );
                }
            }

            for (raw_name, raw_value) in element.attrs() {
                let name = raw_name.to_ascii_lowercase();
                let value = raw_value.trim();

                if name.starts_with("on") {
                    push_unique(
                        &mut v.errors,
                        format!("Blocked inline event handler attribute \"{name}\" found."),
                    );
                }

                if name == "srcdoc" {
                    push_unique(&mut v.errors, "Blocked \"srcdoc\" attribute found.".into());
                }

                if URL_ATTRS.contains(&name.as_str()) {
                    // Strip control chars and spaces the way browsers do before
                    // scheme-matching, so "jav\tascript:" cannot sneak through.
                    let normalized: String = value
                        .chars()
                        .filter(|c| *c > '\u{20}')
                        .collect::<String>()
                        .to_ascii_lowercase();
                    if BLOCKED_PROTOCOLS.iter().any(|p| normalized.starts_with(p)) {
                        push_unique(
                            &mut v.errors,
                            format!("Blocked unsafe URL in \"{name}\" attribute."),
                        );
                    }
                }

                if name == "style" && has_unsafe_css(value) {
                    push_unique(&mut v.errors, "Blocked unsafe inline CSS.".into());
                }
            }

            if tag == "meta" {
                let refresh = element
                    .attrs()
                    .find(|(name, _)| name.eq_ignore_ascii_case("http-equiv"))
                    .map(|(_, value)| value.trim().eq_ignore_ascii_case("refresh"))
                    .unwrap_or(false);
                if refresh {
                    push_unique(&mut v.errors, "Blocked meta refresh tag found.".into());
                }
            }

            // Images are the one external resource the serving CSP allows
            // (img-src https: data:), so record which hosts a draft pulls from.
            if tag == "img" {
                let src = element
                    .attrs()
                    .find(|(name, _)| name.eq_ignore_ascii_case("src"));
                if let Some(host) = src.and_then(|(_, value)| external_host(value)) {
                    if !image_hosts.contains(&host) {
                        image_hosts.push(host);
                    }
                }
            }

            if tag == "title" && v.title.is_none() {
                let text: String = node
                    .descendants()
                    .filter_map(|n| match n.value() {
                        Node::Text(t) => Some(t.text.to_string()),
                        _ => None,
                    })
                    .collect();
                let trimmed: String = text.trim().chars().take(140).collect();
                if !trimmed.is_empty() {
                    v.title = Some(trimmed);
                }
            }
        }

        if depth >= MAX_DEPTH {
            too_deep = true;
            continue;
        }
        for child in node.children() {
            stack.push((child, depth + 1));
        }
    }

    if too_deep {
        push_unique(
            &mut v.errors,
            format!("HTML is nested more than {MAX_DEPTH} levels deep."),
        );
    }

    if v.title.is_none() {
        push_unique(
            &mut v.warnings,
            "No <title> found; Keryx will use a generic title.".into(),
        );
    }

    image_hosts.sort();
    v.external_image_hosts = image_hosts;
    v
}

fn has_unsafe_css(value: &str) -> bool {
    let squished: String = value
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    squished.contains("expression(")
        || squished.contains("behavior:")
        || squished.contains("url(javascript:")
}

/// Lowercased host of an absolute http(s) (or protocol-relative) URL, or None
/// for relative paths, data: URIs, and anything unparseable.
fn external_host(value: &str) -> Option<String> {
    let raw = value.trim();
    if raw.is_empty() {
        return None;
    }
    let candidate = if raw.starts_with("//") {
        format!("https:{raw}")
    } else {
        raw.to_string()
    };
    let url = Url::parse(&candidate).ok()?;
    if url.scheme() == "http" || url.scheme() == "https" {
        url.host_str().map(|h| h.to_ascii_lowercase())
    } else {
        None
    }
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if !list.contains(&value) {
        list.push(value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn validate(html: &str) -> Validation {
        validate_html(html, DEFAULT_MAX_HTML_BYTES)
    }

    #[test]
    fn accepts_plain_document_with_title() {
        let v = validate("<!doctype html><html><head><title>My Plan</title></head><body><h1>Hi</h1></body></html>");
        assert!(v.ok(), "errors: {:?}", v.errors);
        assert_eq!(v.title.as_deref(), Some("My Plan"));
        assert!(v.warnings.is_empty());
    }

    #[test]
    fn allows_inline_classic_script() {
        let v = validate("<title>t</title><script>console.log(1)</script>");
        assert!(v.ok(), "errors: {:?}", v.errors);
        assert!(v.has_inline_script);
    }

    #[test]
    fn rejects_external_and_module_scripts() {
        let v = validate(
            "<script src=\"https://evil.example/x.js\"></script><script type=\"module\">1</script>",
        );
        assert!(v
            .errors
            .iter()
            .any(|e| e.contains("External script sources")));
        assert!(v
            .errors
            .iter()
            .any(|e| e.contains("Unsupported script type")));
    }

    #[test]
    fn rejects_blocked_tags_and_handlers() {
        let v =
            validate("<form action=\"/x\"></form><iframe></iframe><div onclick=\"x()\">hi</div>");
        assert!(v.errors.iter().any(|e| e.contains("<form>")));
        assert!(v.errors.iter().any(|e| e.contains("<iframe>")));
        assert!(v.errors.iter().any(|e| e.contains("onclick")));
    }

    #[test]
    fn rejects_javascript_urls_even_obfuscated() {
        let v = validate("<a href=\"jav\tascript:alert(1)\">x</a>");
        assert!(v.errors.iter().any(|e| e.contains("Blocked unsafe URL")));
    }

    #[test]
    fn rejects_meta_refresh() {
        let v = validate("<meta http-equiv=\"Refresh\" content=\"0;url=https://x\">");
        assert!(v.errors.iter().any(|e| e.contains("meta refresh")));
    }

    #[test]
    fn collects_external_image_hosts() {
        let v = validate("<title>t</title><img src=\"https://CDN.Example.com/a.png\"><img src=\"/local.png\"><img src=\"data:image/png;base64,xx\">");
        assert!(v.ok(), "errors: {:?}", v.errors);
        assert_eq!(v.external_image_hosts, vec!["cdn.example.com".to_string()]);
    }

    #[test]
    fn rejects_empty_and_oversized() {
        assert!(!validate("   ").ok());
        let big = format!("<title>t</title>{}", "a".repeat(DEFAULT_MAX_HTML_BYTES));
        assert!(validate(&big)
            .errors
            .iter()
            .any(|e| e.contains("maximum is")));
    }
}
