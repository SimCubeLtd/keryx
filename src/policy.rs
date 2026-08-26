//! Upload-time HTML safety policy.
//!
//! Uploaded HTML may contain inline classic JavaScript. External script
//! sources, module scripts, inline event-handler attributes, unsafe URL
//! protocols, forms, iframes/embeds, `<link>`/`<base>`, srcdoc, and
//! meta-refresh redirects are rejected. Once stored, a draft is served
//! verbatim.
//!
//! Two of those rules can be relaxed per-server via [`PolicyOptions`]
//! (`keryx serve --allow-font-links` / `--allow-safe-handlers`), which the CLI
//! mirrors from `GET /api/me` so client-side validation matches the server it
//! is uploading to.

use scraper::node::Node;
use scraper::Html;
use serde::{Deserialize, Serialize};
use url::Url;

pub const DEFAULT_MAX_HTML_BYTES: usize = 512 * 1024;

/// Hosts a `<link>` may point at when `allow_font_links` is on. Both are
/// needed for the Google Fonts idiom: the CSS comes from `googleapis`, the
/// font files it references come from `gstatic`.
pub const FONT_HOSTS: &[&str] = &["fonts.googleapis.com", "fonts.gstatic.com"];

/// `rel` values that only fetch a stylesheet or warm a connection. A `<link>`
/// with any other `rel` stays blocked even under `allow_font_links`.
const FONT_LINK_RELS: &[&str] = &["stylesheet", "preconnect", "dns-prefetch", "preload"];

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

/// Script types the HTML spec classifies as data blocks: the parser keeps the
/// element but the UA never executes it. `module`, `importmap` and
/// `speculationrules` are deliberately absent — those are active.
const DATA_SCRIPT_TYPES: &[&str] = &["application/json", "application/ld+json"];

// Far above any real document (browsers themselves flatten around 512).
const MAX_DEPTH: usize = 512;

/// Server-configured limits and opt-in relaxations. Serialized on `/api/me` so
/// `keryx upload` validates against the same rules the server will apply.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct PolicyOptions {
    pub max_html_bytes: usize,
    /// Permit `<link>` elements that point only at [`FONT_HOSTS`].
    pub allow_font_links: bool,
    /// Permit inline `on*` handlers whose body is assignment-only (see
    /// [`is_assignment_only`]).
    pub allow_safe_handlers: bool,
}

impl Default for PolicyOptions {
    fn default() -> Self {
        Self {
            max_html_bytes: DEFAULT_MAX_HTML_BYTES,
            allow_font_links: false,
            allow_safe_handlers: false,
        }
    }
}

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

pub fn validate_html(html: &str, options: &PolicyOptions) -> Validation {
    let mut v = Validation::default();

    if html.trim().is_empty() {
        push_unique(&mut v.errors, "HTML document is empty.".into());
        return v;
    }

    let max_bytes = options.max_html_bytes;
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

            if BLOCKED_TAGS.contains(&tag.as_str())
                && !(tag == "link" && options.allow_font_links && is_font_link(element))
            {
                push_unique(&mut v.errors, format!("Blocked <{tag}> tag found."));
            }

            if tag == "script" {
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
                if ALLOWED_SCRIPT_TYPES.contains(&script_type.as_str()) {
                    v.has_inline_script = true;
                } else if !DATA_SCRIPT_TYPES.contains(&script_type.as_str()) {
                    push_unique(
                        &mut v.errors,
                        format!("Unsupported script type \"{script_type}\" found."),
                    );
                }
            }

            for (raw_name, raw_value) in element.attrs() {
                let name = raw_name.to_ascii_lowercase();
                let value = raw_value.trim();

                if name.starts_with("on")
                    && !(options.allow_safe_handlers && is_assignment_only(value))
                {
                    push_unique(
                        &mut v.errors,
                        format!("Blocked inline event handler attribute \"{name}\" found."),
                    );
                }

                if name == "srcdoc" {
                    push_unique(&mut v.errors, "Blocked \"srcdoc\" attribute found.".into());
                }

                if URL_ATTRS.contains(&name.as_str()) && has_blocked_protocol(value) {
                    push_unique(
                        &mut v.errors,
                        format!("Blocked unsafe URL in \"{name}\" attribute."),
                    );
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

/// A `<link>` is acceptable under `allow_font_links` only when every `rel`
/// token is connection- or stylesheet-shaped and its `href` resolves to a
/// known font host. No href means nothing to fetch and no reason to allow it.
fn is_font_link(element: &scraper::node::Element) -> bool {
    let rel_ok = element
        .attrs()
        .find(|(name, _)| name.eq_ignore_ascii_case("rel"))
        .is_some_and(|(_, value)| {
            let tokens: Vec<&str> = value.split_whitespace().collect();
            !tokens.is_empty()
                && tokens.iter().all(|token| {
                    FONT_LINK_RELS
                        .iter()
                        .any(|allowed| token.eq_ignore_ascii_case(allowed))
                })
        });
    if !rel_ok {
        return false;
    }
    element
        .attrs()
        .find(|(name, _)| name.eq_ignore_ascii_case("href"))
        .and_then(|(_, value)| external_host(value))
        .is_some_and(|host| FONT_HOSTS.contains(&host.as_str()))
}

/// True when an inline handler body is nothing but `;`-separated assignments of
/// literals or property paths — the shape of the async-CSS idiom
/// `this.media='all'`. Function calls, subscripts, and markup are all out, so a
/// permitted handler can mutate properties but cannot invoke anything.
fn is_assignment_only(value: &str) -> bool {
    let normalized = normalize_url(value);
    if value.is_empty()
        || BLOCKED_PROTOCOLS
            .iter()
            .any(|protocol| normalized.contains(protocol))
    {
        return false;
    }
    // `<` would let an assignment smuggle markup (and its own handlers) into an
    // innerHTML sink; `(`/`[` are the only ways to call or index.
    if value.contains(['(', ')', '[', ']', '<', '>', '{', '}', '`']) {
        return false;
    }

    let statements: Vec<&str> = value
        .split(';')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    !statements.is_empty()
        && statements.iter().all(|statement| {
            let mut sides = statement.splitn(2, '=');
            match (sides.next(), sides.next()) {
                (Some(target), Some(source)) => {
                    is_property_path(target.trim()) && is_literal_or_path(source.trim())
                }
                _ => false,
            }
        })
}

/// `this`, `foo`, `this.style.display` — dotted identifiers only.
fn is_property_path(value: &str) -> bool {
    !value.is_empty()
        && value.split('.').all(|segment| {
            let mut chars = segment.chars();
            chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_' || c == '$')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '$')
        })
}

fn is_literal_or_path(value: &str) -> bool {
    let quoted = (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
        || (value.starts_with('"') && value.ends_with('"') && value.len() >= 2);
    if quoted {
        // Reject a second quote pair, which would mean concatenation or a
        // trailing expression we have not parsed.
        let quote = value.chars().next().unwrap_or('\'');
        return value.matches(quote).count() == 2;
    }
    value.parse::<f64>().is_ok() || is_property_path(value)
}

/// Strip control chars and spaces the way browsers do before scheme-matching,
/// so "jav\tascript:" cannot sneak through.
fn normalize_url(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|c| *c > '\u{20}')
        .collect::<String>()
        .to_ascii_lowercase()
}

/// A URL attribute is unsafe when it *starts* with a blocked scheme; a blocked
/// scheme appearing later is just query-string text.
fn has_blocked_protocol(value: &str) -> bool {
    let normalized = normalize_url(value);
    BLOCKED_PROTOCOLS
        .iter()
        .any(|protocol| normalized.starts_with(protocol))
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
        validate_html(html, &PolicyOptions::default())
    }

    fn validate_with(html: &str, options: PolicyOptions) -> Validation {
        validate_html(html, &options)
    }

    const FONT_LINKS: &str = "<title>t</title>\
        <link rel=\"preconnect\" href=\"https://fonts.gstatic.com\" crossorigin>\
        <link href=\"https://fonts.googleapis.com/css2?family=JetBrains+Mono\" rel=\"stylesheet\" \
        media=\"print\" onload=\"this.media='all'\">";

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
    fn allows_inert_json_data_blocks_but_not_active_types() {
        let v = validate("<title>t</title><script type=\"application/json\">{\"a\":1}</script>");
        assert!(v.ok(), "errors: {:?}", v.errors);
        assert!(!v.has_inline_script, "data blocks never execute");

        let v = validate("<script type=\"importmap\">{}</script>");
        assert!(v
            .errors
            .iter()
            .any(|e| e.contains("Unsupported script type")));
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
    fn font_links_need_the_flag() {
        let strict = validate(FONT_LINKS);
        assert!(strict.errors.iter().any(|e| e.contains("<link>")));
        assert!(strict.errors.iter().any(|e| e.contains("onload")));

        let relaxed = validate_with(
            FONT_LINKS,
            PolicyOptions {
                allow_font_links: true,
                allow_safe_handlers: true,
                ..PolicyOptions::default()
            },
        );
        assert!(relaxed.ok(), "errors: {:?}", relaxed.errors);
    }

    #[test]
    fn font_link_flag_only_covers_font_hosts_and_fetch_rels() {
        let options = PolicyOptions {
            allow_font_links: true,
            ..PolicyOptions::default()
        };
        for html in [
            "<link rel=\"stylesheet\" href=\"https://evil.example/x.css\">",
            "<link rel=\"stylesheet\" href=\"/local.css\">",
            "<link rel=\"import\" href=\"https://fonts.googleapis.com/x\">",
            "<link rel=\"stylesheet\">",
        ] {
            let v = validate_with(html, options);
            assert!(
                v.errors.iter().any(|e| e.contains("<link>")),
                "should still block: {html}"
            );
        }
        // <base> is never unblocked by the font flag.
        let v = validate_with("<base href=\"https://fonts.googleapis.com/\">", options);
        assert!(v.errors.iter().any(|e| e.contains("<base>")));
    }

    #[test]
    fn safe_handler_flag_permits_assignments_only() {
        let options = PolicyOptions {
            allow_safe_handlers: true,
            ..PolicyOptions::default()
        };
        for body in [
            "this.media='all'",
            "this.style.display='none'; x.y=1",
            "a=b",
        ] {
            let html = format!("<title>t</title><span onload=\"{body}\"></span>");
            let v = validate_with(&html, options);
            assert!(v.ok(), "{body} should pass: {:?}", v.errors);
        }
        for body in [
            "alert(1)",
            "this.media=go()",
            "window['ev'+'al']=1",
            "this.innerHTML='&lt;img src=x&gt;'",
            "this.href='javascript:alert(1)'",
            "fetch",
            "",
        ] {
            let html = format!("<title>t</title><span onclick=\"{body}\"></span>");
            let v = validate_with(&html, options);
            assert!(
                v.errors.iter().any(|e| e.contains("onclick")),
                "{body} should be blocked"
            );
        }
    }

    #[test]
    fn rejects_javascript_urls_even_obfuscated() {
        let v = validate("<a href=\"jav\tascript:alert(1)\">x</a>");
        assert!(v.errors.iter().any(|e| e.contains("Blocked unsafe URL")));
    }

    #[test]
    fn allows_query_strings_that_merely_mention_a_blocked_scheme() {
        let v = validate("<title>t</title><a href=\"https://ok.example/?q=javascript:1\">x</a>");
        assert!(v.ok(), "errors: {:?}", v.errors);
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
        let raised = validate_with(
            &big,
            PolicyOptions {
                max_html_bytes: DEFAULT_MAX_HTML_BYTES * 4,
                ..PolicyOptions::default()
            },
        );
        assert!(raised.ok(), "errors: {:?}", raised.errors);
    }
}
