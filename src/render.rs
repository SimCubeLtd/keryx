//! Server-rendered pages: the dashboard (which replaces the old auth'd web
//! dashboard — this is a private self-hosted service) and the 404 page.

use crate::types::DraftSummary;

pub fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn html_page(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>{title}</title>
  <style>
    body {{
      margin: 0;
      background: #f8fafc;
      color: #111827;
      font-family: ui-sans-serif, system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif;
    }}
    .home {{ max-width: 760px; margin: 64px auto; padding: 0 20px; }}
    h1 {{ margin: 0 0 12px; font-size: 40px; line-height: 1.1; }}
    p {{ color: #374151; font-size: 17px; line-height: 1.6; }}
    pre {{
      overflow-x: auto;
      padding: 14px;
      border: 1px solid #d1d5db;
      background: #ffffff;
      border-radius: 6px;
    }}
    ul.drafts {{ list-style: none; margin: 24px 0; padding: 0; }}
    ul.drafts li {{
      padding: 14px;
      border: 1px solid #d1d5db;
      background: #ffffff;
      border-radius: 6px;
      margin-bottom: 10px;
    }}
    ul.drafts .meta {{ color: #6b7280; font-size: 14px; margin-top: 4px; }}
    ul.drafts a {{ color: #1d4ed8; text-decoration: none; font-weight: 600; }}
    ul.drafts a.raw {{ font-weight: 400; font-size: 14px; }}
    .disabled {{ color: #b91c1c; }}
  </style>
</head>
<body>{body}</body>
</html>"#,
        title = escape_html(title),
        body = body
    )
}

pub fn render_dashboard(drafts: &[DraftSummary], base_url: &str) -> String {
    let items = if drafts.is_empty() {
        "<p>No drafts yet. Publish one with: <code>keryx upload ./plan.html</code></p>".to_string()
    } else {
        let rows: String = drafts
            .iter()
            .map(|d| {
                let repo = match (&d.repo_org, &d.repo_name) {
                    (Some(org), Some(name)) => format!("{org}/{name}"),
                    _ => "no repo".to_string(),
                };
                let version = d
                    .latest_version_number
                    .map(|n| format!("v{n}"))
                    .unwrap_or_else(|| "no versions".to_string());
                let disabled = if d.disabled {
                    " · <span class=\"disabled\">disabled</span>"
                } else {
                    ""
                };
                let description = d
                    .description
                    .as_deref()
                    .map(|text| format!("<div class=\"meta\">{}</div>", escape_html(text)))
                    .unwrap_or_default();
                format!(
                    r#"<li>
  <a href="{public}">{title}</a> · <a class="raw" href="{raw}">raw</a>
  {description}
  <div class="meta">{repo} · {version} · {count} version{plural} · updated {updated}{disabled} · <code>{id}</code></div>
</li>"#,
                    public = escape_html(&d.public_url),
                    raw = escape_html(&d.raw_url),
                    title = escape_html(&d.title),
                    description = description,
                    repo = escape_html(&repo),
                    version = version,
                    count = d.version_count,
                    plural = if d.version_count == 1 { "" } else { "s" },
                    updated = escape_html(&d.updated_at),
                    disabled = disabled,
                    id = escape_html(&d.draft_id),
                )
            })
            .collect();
        format!("<ul class=\"drafts\">{rows}</ul>")
    };

    html_page(
        "Keryx",
        &format!(
            r#"<main class="home">
  <h1>Keryx</h1>
  <p>Self-hosted static HTML draft publishing for agents.</p>
  <pre>keryx upload ./plan.html --api-url {base}</pre>
  {items}
  <p>Health: <a href="/healthz">/healthz</a></p>
</main>"#,
            base = escape_html(base_url),
            items = items
        ),
    )
}

pub fn render_not_found() -> String {
    html_page(
        "Draft not found",
        r#"<main class="home">
  <h1>Draft not found</h1>
  <p>The requested draft is unavailable.</p>
</main>"#,
    )
}
