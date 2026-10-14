//! Shared HTML escaping and PDF rendering. Dashboard assets are built in frontend/.

pub mod pdf;

pub fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub fn render_not_found() -> String {
    r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Draft not found</title><style>body{margin:0;padding:64px 20px;background:#f8fafc;color:#111827;font-family:ui-sans-serif,system-ui,sans-serif}main{max-width:760px;margin:auto}h1{font-size:40px}p{color:#475569;font-size:17px}</style></head><body><main><h1>Draft not found</h1><p>The requested draft is unavailable.</p></main></body></html>"#.to_string()
}
