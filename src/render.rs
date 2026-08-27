//! Server-rendered pages for the dashboard and 404 response. The dashboard
//! stays framework-free: Rust renders the durable document and a small inline
//! script adds tab switching, search, selection, theme persistence, and
//! management actions.

use std::collections::BTreeSet;

use crate::types::{Availability, DraftSummary};

const DASHBOARD_CSS: &str = include_str!("../assets/dashboard.css");
const DASHBOARD_JS: &str = include_str!("../assets/dashboard.js");
const KERYX_LOGO: &str = include_str!("../assets/keryx-logo.svg");

pub fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn repository(draft: &DraftSummary) -> Option<String> {
    match (&draft.repo_org, &draft.repo_name) {
        (Some(org), Some(name)) => Some(format!("{org}/{name}")),
        _ => None,
    }
}

fn display_or(value: Option<&str>, fallback: &str) -> String {
    escape_html(value.unwrap_or(fallback))
}

fn render_row(draft: &DraftSummary, selected: bool, management_enabled: bool) -> String {
    let availability = draft.availability();
    let repository = management_enabled.then(|| repository(draft)).flatten();
    let repository_label = if management_enabled {
        repository.as_deref().unwrap_or("Provenance not recorded")
    } else {
        "Protected"
    };
    let branch = if management_enabled {
        draft
            .latest_git_branch
            .as_deref()
            .unwrap_or("Historic upload")
    } else {
        "API key required"
    };
    let description = draft.description.as_deref().unwrap_or("");
    let version = draft.latest_version_number.unwrap_or(0);
    let search = format!(
        "{} {} {} {} {}",
        draft.title, description, draft.draft_id, repository_label, branch
    )
    .to_lowercase();
    let download_name = format!("keryx-{}-v{}.html", draft.draft_id, version);
    let protected_actions = if management_enabled {
        let pdf_url = format!("/api/drafts/{}/pdf", draft.draft_id);
        format!(
            r#"<div class="menu-group">
          <a class="menu-item" href="{pdf_url}" download><span class="menu-icon">↓</span><span><span class="menu-label">Download PDF</span><span class="menu-help">Current immutable version</span></span></a>
          <a class="menu-item" href="{raw_url}" download="{download_name}"><span class="menu-icon">↓</span><span><span class="menu-label">Download HTML</span><span class="menu-help">Exact stored source</span></span></a>
        </div>
        <div class="menu-group">
          <button class="menu-item" type="button" data-action="prune" data-draft-id="{id}" data-title="{title}"><span class="menu-icon">⌫</span><span><span class="menu-label">Prune draft</span><span class="menu-help">Stop serving; retain stored versions</span></span></button>
          <button class="menu-item danger" type="button" data-action="purge" data-draft-id="{id}" data-title="{title}"><span class="menu-icon">×</span><span><span class="menu-label">Force prune…</span><span class="menu-help">Permanently delete every version</span></span></button>
        </div>"#,
            pdf_url = escape_html(&pdf_url),
            raw_url = escape_html(&draft.raw_url),
            download_name = escape_html(&download_name),
            id = escape_html(&draft.draft_id),
            title = escape_html(&draft.title),
        )
    } else {
        format!(
            r#"<div class="menu-group">
          <a class="menu-item" href="{raw_url}" download="{download_name}"><span class="menu-icon">↓</span><span><span class="menu-label">Download HTML</span><span class="menu-help">Exact stored source</span></span></a>
        </div>
        <div class="menu-note">PDF publishing, availability, and prune commands require the authenticated CLI.</div>"#,
            raw_url = escape_html(&draft.raw_url),
            download_name = escape_html(&download_name),
        )
    };

    format!(
        r#"<tr class="draft-row{selected_class}" tabindex="0" role="option" aria-selected="{selected}"{hidden} data-draft-id="{id}" data-title="{title}" data-description="{description}" data-repository="{repository}" data-repo-host="{repo_host}" data-branch="{branch}" data-commit-sha="{commit_sha}" data-commit-subject="{commit_subject}" data-updated="{updated}" data-latest-version="{version}" data-version-count="{version_count}" data-disabled="{disabled}" data-snoozed-until="{snoozed_until}" data-availability="{availability}" data-public-url="{public_url}" data-search="{search}">
  <td><div class="draft-title"><a href="{public_url}" target="_blank" rel="noopener noreferrer">{title}</a></div><div class="draft-description">{description_display}</div></td>
  <td><div class="source{missing_class}">{repository}</div><div class="branch">{branch}</div></td>
  <td><div class="updated" title="{updated}">{updated}</div><div class="version">v{version} · {version_count} {version_word}</div></td>
  <td class="row-action">
    <details class="row-menu">
      <summary aria-label="Actions for {title}">•••</summary>
      <div class="menu">
        <div class="menu-group">
          <a class="menu-item" href="{public_url}" target="_blank" rel="noopener noreferrer"><span class="menu-icon">↗</span><span class="menu-label">Open document</span></a>
          <a class="menu-item" href="{raw_url}"><span class="menu-icon">⌁</span><span class="menu-label">View raw HTML</span></a>
        </div>
        {protected_actions}
      </div>
    </details>
  </td>
</tr>"#,
        selected_class = if selected { " selected" } else { "" },
        selected = selected,
        hidden = if availability == Availability::Active {
            ""
        } else {
            " hidden"
        },
        id = escape_html(&draft.draft_id),
        title = escape_html(&draft.title),
        description = escape_html(description),
        description_display = escape_html(if description.is_empty() {
            "No description supplied"
        } else {
            description
        }),
        repository = escape_html(repository_label),
        repo_host = if management_enabled {
            display_or(draft.repo_host.as_deref(), "Not recorded")
        } else {
            "Protected".into()
        },
        branch = escape_html(branch),
        commit_sha = if management_enabled {
            display_or(draft.latest_git_commit_sha.as_deref(), "Not recorded")
        } else {
            String::new()
        },
        commit_subject = if management_enabled {
            display_or(draft.latest_git_commit_subject.as_deref(), "")
        } else {
            String::new()
        },
        updated = escape_html(&draft.updated_at),
        version = version,
        version_count = draft.version_count,
        version_word = if draft.version_count == 1 {
            "version"
        } else {
            "versions"
        },
        disabled = draft.disabled,
        snoozed_until = display_or(draft.snoozed_until.as_deref(), ""),
        availability = availability.as_str(),
        public_url = escape_html(&draft.public_url),
        raw_url = escape_html(&draft.raw_url),
        search = escape_html(&search),
        missing_class = if repository.is_none() { " missing" } else { "" },
        protected_actions = protected_actions,
    )
}

fn render_detail(draft: Option<&DraftSummary>, management_enabled: bool) -> String {
    let Some(draft) = draft else {
        return r#"<aside class="detail empty" id="draft-detail"><div><h2>No drafts yet</h2><p>Publish one with <code>keryx upload ./plan.html</code>.</p></div></aside>"#.to_string();
    };
    let availability = draft.availability();
    let repository = if management_enabled {
        repository(draft).unwrap_or_else(|| "Provenance not recorded".into())
    } else {
        "Protected".into()
    };
    let branch = if management_enabled {
        draft.latest_git_branch.as_deref().unwrap_or("Not recorded")
    } else {
        "API key required"
    };
    let commit = if management_enabled {
        draft
            .latest_git_commit_subject
            .as_deref()
            .or(draft.latest_git_commit_sha.as_deref())
            .unwrap_or("Not recorded")
    } else {
        "Protected"
    };
    let version = draft.latest_version_number.unwrap_or(0);
    let unavailable = availability == Availability::Disabled;
    let detail_actions = if management_enabled {
        format!(
            r#"<a class="button primary" id="detail-open" href="{public_url}" target="_blank" rel="noopener noreferrer" aria-disabled="{unavailable}">Open document ↗</a>
    <a class="button" id="detail-pdf" href="/api/drafts/{id}/pdf" download aria-disabled="{unavailable}">Download PDF</a>
    <button class="button amber" id="action-snooze" type="button" data-availability-action="snooze" hidden>Snooze</button>
    <button class="button amber" id="action-unsnooze" type="button" data-availability-action="unsnooze" hidden>Unsnooze</button>
    <button class="button" id="action-disable" type="button" data-availability-action="disable" hidden>Disable</button>
    <button class="button" id="action-enable" type="button" data-availability-action="enable" hidden>Enable</button>"#,
            public_url = escape_html(&draft.public_url),
            unavailable = unavailable,
            id = escape_html(&draft.draft_id),
        )
    } else {
        format!(
            r#"<a class="button primary" id="detail-open" href="{public_url}" target="_blank" rel="noopener noreferrer" aria-disabled="{unavailable}">Open document ↗</a>
    <span class="menu-note">Snooze, disable, and enable require the authenticated CLI.</span>"#,
            public_url = escape_html(&draft.public_url),
            unavailable = unavailable,
        )
    };

    format!(
        r#"<aside class="detail" id="draft-detail" aria-label="Selected draft details">
  <div class="detail-empty"><h2>No matching drafts</h2><p>Adjust the search or pick another tab.</p></div>
  <div class="detail-content">
  <div class="detail-kicker"><span class="status {availability}" id="detail-status">{status}</span> Selected draft</div>
  <h2 id="detail-title">{title}</h2>
  <p class="detail-copy" id="detail-description">{description}</p>
  <p class="detail-availability" id="detail-availability" hidden></p>
  <div class="detail-actions">
    {detail_actions}
  </div>
  <div class="meta-grid">
    <div class="meta-cell"><div class="meta-key">Origin</div><div class="meta-value mono" id="detail-origin">{repo_host}</div></div>
    <div class="meta-cell"><div class="meta-key">Repository</div><div class="meta-value" id="detail-repository">{repository}</div></div>
    <div class="meta-cell"><div class="meta-key">Latest branch</div><div class="meta-value mono" id="detail-branch">{branch}</div></div>
    <div class="meta-cell"><div class="meta-key">Draft ID</div><div class="meta-value mono" id="detail-id">{id}</div></div>
    <div class="meta-cell"><div class="meta-key">Updated</div><div class="meta-value" id="detail-updated">{updated}</div></div>
    <div class="meta-cell"><div class="meta-key">Current</div><div class="meta-value" id="detail-version">Version {version} of {version_count}</div></div>
    <div class="meta-cell"><div class="meta-key">Latest commit</div><div class="meta-value" id="detail-commit">{commit}</div></div>
  </div>
  <div class="history-head"><h3>Version history</h3><span class="history-state" id="history-state">{history_state}</span></div>
  <div id="version-history"></div>
  </div>
</aside>"#,
        availability = availability.as_str(),
        status = status_label(availability),
        title = escape_html(&draft.title),
        description = display_or(draft.description.as_deref(), "No description supplied."),
        detail_actions = detail_actions,
        id = escape_html(&draft.draft_id),
        repo_host = if management_enabled {
            display_or(draft.repo_host.as_deref(), "Not recorded")
        } else {
            "Protected".into()
        },
        repository = escape_html(&repository),
        branch = escape_html(branch),
        updated = escape_html(&draft.updated_at),
        version = version,
        version_count = draft.version_count,
        commit = escape_html(commit),
        history_state = if management_enabled {
            "Loading…"
        } else {
            "Use authenticated CLI"
        },
    )
}

fn status_label(availability: Availability) -> &'static str {
    match availability {
        Availability::Active => "Active",
        Availability::Snoozed => "Snoozed",
        Availability::Disabled => "Disabled",
    }
}

pub fn render_dashboard(
    drafts: &[DraftSummary],
    _base_url: &str,
    management_enabled: bool,
) -> String {
    // The dashboard opens on Active, so the first active draft is selected.
    let first_active = drafts
        .iter()
        .find(|draft| draft.availability() == Availability::Active);
    let selected_id = first_active.map(|draft| draft.draft_id.as_str());
    let rows = drafts
        .iter()
        .map(|draft| {
            render_row(
                draft,
                Some(draft.draft_id.as_str()) == selected_id,
                management_enabled,
            )
        })
        .collect::<String>();
    let repositories = if management_enabled {
        drafts
            .iter()
            .filter_map(repository)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .map(|repository| {
                format!(
                    "<option value=\"{value}\">{value}</option>",
                    value = escape_html(&repository)
                )
            })
            .collect::<String>()
    } else {
        String::new()
    };
    let count_of = |state: Availability| {
        drafts
            .iter()
            .filter(|draft| draft.availability() == state)
            .count()
    };
    let active_count = count_of(Availability::Active);
    let snoozed_count = count_of(Availability::Snoozed);
    let disabled_count = count_of(Availability::Disabled);
    let empty_hidden = if active_count == 0 { "" } else { " hidden" };
    let repo_filter = if management_enabled {
        format!(
            r#"<select class="repo-filter" id="repo-filter" aria-label="Filter by repository"><option value="">All repositories</option>{repositories}</select>"#
        )
    } else {
        r#"<span class="protected-note">Git provenance and management require the authenticated CLI</span>"#.into()
    };
    let management_note = if management_enabled {
        String::new()
    } else {
        r#"<span class="management-mode">Public view · management via authenticated CLI</span>"#
            .into()
    };
    // Hidden until the script confirms a secure context with push support.
    let notify_menu = if management_enabled {
        r#"<details class="notify-menu" id="notify-menu" hidden>
        <summary id="notify-summary">Notifications off</summary>
        <div class="menu notify-panel">
          <div class="notify-state" id="notify-state"></div>
          <div class="menu-group">
            <label class="notify-option"><input type="checkbox" data-event="published" checked> Plan published</label>
            <label class="notify-option"><input type="checkbox" data-event="revised" checked> Plan revised</label>
            <label class="notify-option"><input type="checkbox" data-event="woke" checked> Plan woke</label>
            <label class="notify-option"><input type="checkbox" data-event="enabled" checked> Plan enabled</label>
            <label class="notify-option"><input type="checkbox" data-event="disabled" checked> Plan disabled</label>
          </div>
          <div class="notify-actions"><button class="button primary" id="notify-enable" type="button">Enable on this device</button><button class="button" id="notify-disable" type="button" hidden>Turn off</button></div>
        </div>
      </details>"#
    } else {
        ""
    };

    format!(
        r#"<!doctype html>
<html lang="en" data-theme="system">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Keryx</title>
  <meta name="theme-color" content="{theme_color}">
  <link rel="manifest" href="/manifest.webmanifest">
  <link rel="icon" type="image/png" sizes="192x192" href="/pwa-icon-192.png">
  <link rel="apple-touch-icon" href="/pwa-icon-192.png">
  <script>(function(){{try{{var t=localStorage.getItem("keryx-theme")||"system";if(["system","light","dark"].indexOf(t)<0)t="system";document.documentElement.dataset.theme=t}}catch(_){{}}}})();</script>
  <style>{css}</style>
</head>
<body data-management-enabled="{management_enabled}">
  <header class="topbar">
    <div class="brand"><span class="brand-mark">{KERYX_LOGO}</span>Keryx</div>
    <div class="topbar-tools">
      {management_note}
      {notify_menu}
      <button class="button install" id="install-app" type="button" hidden>Install Keryx</button>
      <a class="health" href="/healthz">Server healthy</a>
      <label class="theme-control"><span>Theme</span><select id="theme-select" aria-label="Theme"><option value="system">System</option><option value="light">Light</option><option value="dark">Dark</option></select></label>
    </div>
  </header>
  <main class="dashboard">
    <section class="workspace">
      <div class="workspace-head">
        <div class="head-row">
          <div><h1>Drafts</h1><p id="result-count">{active_count} active</p></div>
          <label class="search">
            <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" aria-hidden="true"><circle cx="11" cy="11" r="7"></circle><path d="m20 20-3.5-3.5"></path></svg>
            <input id="draft-search" type="search" placeholder="Search active drafts…" aria-label="Search drafts">
            <span class="kbd">/</span>
          </label>
        </div>
        <div class="filters">
          <div class="tabs" role="tablist" aria-label="Availability">
            <button class="tab" type="button" role="tab" data-view="active" aria-selected="true">Active <span class="count" data-count="active">{active_count}</span></button>
            <button class="tab snooze" type="button" role="tab" data-view="snoozed" aria-selected="false">Snoozed <span class="count" data-count="snoozed">{snoozed_count}</span></button>
            <button class="tab off" type="button" role="tab" data-view="disabled" aria-selected="false">Disabled <span class="count" data-count="disabled">{disabled_count}</span></button>
          </div>
          {repo_filter}
          <label class="sort">Sort<select id="draft-sort" aria-label="Sort drafts"><option value="updated">Recently updated</option><option value="oldest">Oldest updated</option><option value="title">Title</option><option value="versions">Most versions</option></select></label>
        </div>
      </div>
      <div class="split">
        <div class="master">
          <div class="table-wrap"><table class="draft-table"><colgroup><col><col><col><col></colgroup><thead><tr><th>Document</th><th>Source</th><th id="time-column">Updated</th><th></th></tr></thead><tbody id="draft-rows" role="listbox">{rows}</tbody></table></div>
          <div class="empty-results" id="empty-results"{empty_hidden}><strong id="empty-title">No active drafts</strong><span id="empty-hint">Publish one with keryx upload ./plan.html</span></div>
        </div>
        {detail}
      </div>
    </section>
  </main>
  <dialog id="prune-dialog">
    <div class="dialog-body">
      <div class="dialog-mark">!</div>
      <h2 id="dialog-title">Prune this draft?</h2>
      <p id="dialog-description"></p>
      <div class="dialog-summary"><strong id="dialog-draft-title"></strong><span id="dialog-draft-id"></span></div>
      <div class="dialog-warning" id="dialog-warning"></div>
      <div class="dialog-actions"><button class="button" id="cancel-action" type="button">Cancel</button><button class="button" id="confirm-action" type="button">Prune draft</button></div>
    </div>
  </dialog>
  <dialog id="snooze-dialog">
    <div class="dialog-body">
      <h2>Snooze this draft</h2>
      <p>Move it out of Active until a wake time. Public, raw, versioned, and PDF links keep working while it sleeps.</p>
      <div class="dialog-summary"><strong id="snooze-draft-title"></strong><span id="snooze-draft-id"></span></div>
      <div class="snooze-presets" aria-label="Wake time presets">
        <button class="preset" type="button" data-preset="1h">In 1 hour</button>
        <button class="preset" type="button" data-preset="4h">In 4 hours</button>
        <button class="preset" type="button" data-preset="tomorrow">Tomorrow 09:00</button>
        <button class="preset" type="button" data-preset="monday">Monday 09:00</button>
      </div>
      <label class="snooze-custom">Custom wake time<input type="datetime-local" id="snooze-until" step="60"></label>
      <p class="snooze-preview" id="snooze-preview"></p>
      <div class="dialog-actions"><button class="button" id="snooze-cancel" type="button">Cancel</button><button class="button primary" id="snooze-confirm" type="button">Snooze</button></div>
    </div>
  </dialog>
  <div class="toast" id="toast" role="status" hidden></div>
  <script>{js}</script>
</body>
</html>"#,
        css = DASHBOARD_CSS,
        js = DASHBOARD_JS,
        theme_color = "#2563eb",
        active_count = active_count,
        snoozed_count = snoozed_count,
        disabled_count = disabled_count,
        repo_filter = repo_filter,
        management_note = management_note,
        notify_menu = notify_menu,
        management_enabled = management_enabled,
        rows = rows,
        empty_hidden = empty_hidden,
        detail = render_detail(first_active.or(drafts.first()), management_enabled),
    )
}

pub fn render_not_found() -> String {
    r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>Draft not found</title><style>body{margin:0;padding:64px 20px;background:#f8fafc;color:#111827;font-family:ui-sans-serif,system-ui,sans-serif}main{max-width:760px;margin:auto}h1{font-size:40px}p{color:#475569;font-size:17px}</style></head><body><main><h1>Draft not found</h1><p>The requested draft is unavailable.</p></main></body></html>"#.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft() -> DraftSummary {
        DraftSummary {
            draft_id: "abc123def456".into(),
            title: "Dashboard <direction>".into(),
            description: Some("Selected mock".into()),
            repo_org: Some("SimCubeLtd".into()),
            repo_name: Some("keryx".into()),
            repo_host: Some("github.com".into()),
            latest_version_number: Some(4),
            version_count: 4,
            created_at: "2026-08-20T10:00:00.000Z".into(),
            updated_at: "2026-08-26T22:00:00.000Z".into(),
            latest_version_at: Some("2026-08-26T22:00:00.000Z".into()),
            latest_git_branch: Some("feat/dashboard-ui".into()),
            latest_git_commit_sha: Some("1234567890abcdef".into()),
            latest_git_commit_subject: Some("Build the dashboard".into()),
            latest_git_dirty: Some(false),
            disabled: false,
            snoozed_until: None,
            public_url: "https://keryx.test/d/abc123def456".into(),
            raw_url: "https://keryx.test/d/abc123def456/raw".into(),
        }
    }

    #[test]
    fn dashboard_renders_the_selected_workspace_and_management_actions() {
        let html = render_dashboard(&[draft()], "https://keryx.test", true);

        assert!(html.contains("data-draft-id=\"abc123def456\""));
        assert!(html.contains("feat/dashboard-ui"));
        assert!(html.contains("Download PDF"));
        assert!(html.contains("Download HTML"));
        assert!(html.contains("Prune draft"));
        assert!(html.contains("Force prune"));
        assert!(html.contains("data-availability-action=\"snooze\""));
        assert!(html.contains("data-availability-action=\"disable\""));
        assert!(html.contains("id=\"snooze-dialog\""));
        assert!(html.contains("<link rel=\"manifest\" href=\"/manifest.webmanifest\">"));
        assert!(html.contains("id=\"install-app\" type=\"button\" hidden"));
        assert!(html.contains("id=\"notify-menu\" hidden"));
        assert!(html.contains("data-event=\"woke\""));
        assert!(html.contains("<option value=\"system\">System</option>"));
        assert!(html.contains("<svg"));
        assert!(html.contains("Keryx Dispatch"));
        assert!(html.contains("href=\"https://keryx.test/d/abc123def456\" target=\"_blank\" rel=\"noopener noreferrer\""));
        assert!(html.contains("item.href = \"/d/\" + encodeURIComponent(draftId) + \"/v/\""));
        assert!(html.contains("Load \" + remaining + \" older \""));
        assert!(!html.contains("Dashboard <direction>"));
        assert!(html.contains("Dashboard &lt;direction&gt;"));
    }

    #[test]
    fn dashboard_opens_on_active_and_drops_the_old_filters() {
        let snoozed = DraftSummary {
            draft_id: "snoozed00001".into(),
            title: "Release checklist".into(),
            snoozed_until: Some("2099-01-01T09:00:00.000Z".into()),
            ..draft()
        };
        let disabled = DraftSummary {
            draft_id: "disabled0001".into(),
            title: "Retired plan".into(),
            disabled: true,
            ..draft()
        };
        let html = render_dashboard(&[snoozed, disabled, draft()], "https://keryx.test", true);

        assert!(html.contains("data-view=\"active\" aria-selected=\"true\""));
        assert!(html.contains("data-view=\"snoozed\" aria-selected=\"false\""));
        assert!(html.contains("data-view=\"disabled\" aria-selected=\"false\""));
        assert!(html.contains("data-count=\"active\">1<"));
        assert!(html.contains("data-count=\"snoozed\">1<"));
        assert!(html.contains("data-count=\"disabled\">1<"));
        assert!(html.contains("<p id=\"result-count\">1 active</p>"));
        assert!(!html.contains("data-filter="));
        assert!(!html.contains("Updated this week"));
        assert!(!html.contains("Multi-version"));
        assert!(!html.contains("Provenance not recorded <span"));

        // Only the active draft is visible and selected on first paint.
        assert!(html.contains("aria-selected=\"false\" hidden data-draft-id=\"snoozed00001\""));
        assert!(html.contains(
            "data-snoozed-until=\"2099-01-01T09:00:00.000Z\" data-availability=\"snoozed\""
        ));
        assert!(html.contains("aria-selected=\"false\" hidden data-draft-id=\"disabled0001\""));
        assert!(html.contains("data-availability=\"disabled\""));
        assert!(html.contains("aria-selected=\"true\" data-draft-id=\"abc123def456\""));
        assert!(html.contains("<h2 id=\"detail-title\">Dashboard &lt;direction&gt;</h2>"));
        assert!(html.contains("<span class=\"status active\" id=\"detail-status\">Active</span>"));
    }

    #[test]
    fn protected_dashboard_redacts_provenance_and_omits_authenticated_actions() {
        let html = render_dashboard(&[draft()], "https://keryx.test", false);

        assert!(html.contains("Public view · management via authenticated CLI"));
        assert!(html.contains("Use authenticated CLI"));
        assert!(html.contains("data-view=\"snoozed\""));
        assert!(!html.contains("feat/dashboard-ui"));
        assert!(!html.contains("1234567890abcdef"));
        assert!(!html.contains("SimCubeLtd/keryx"));
        assert!(!html.contains("data-action=\"prune\""));
        assert!(!html.contains("data-action=\"purge\""));
        assert!(!html.contains("data-availability-action="));
        assert!(!html.contains("id=\"notify-menu\""));
        assert!(!html.contains("id=\"repo-filter\""));
        assert!(!html.contains("href=\"/api/drafts/abc123def456/pdf\""));
        assert!(html.contains("Download HTML"));
        assert!(html.contains("Snooze, disable, and enable require the authenticated CLI."));
    }
}
