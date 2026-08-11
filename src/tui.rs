//! Interactive draft browser. Talks to the same HTTP API as the CLI, so it
//! works against a local or remote keryx server.
//!
//! Keys — list: j/k or arrows move, Enter versions, o open in browser,
//! y show raw URL, d delete (with confirm), r refresh, q quit.
//! Versions: j/k move, o open that version, Esc/q back.

use anyhow::Result;
use clap::Args;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::cli::time_ago;
use crate::client::Api;
use crate::types::{DraftDetail, DraftSummary};

#[derive(Args, Debug)]
pub struct TuiArgs {
    /// Override the Keryx API base URL
    #[arg(long)]
    pub api_url: Option<String>,
}

enum View {
    List,
    Versions {
        detail: Box<DraftDetail>,
        selected: usize,
    },
    ConfirmDelete {
        purge: bool,
    },
}

struct App {
    api: Api,
    drafts: Vec<DraftSummary>,
    selected: usize,
    view: View,
    status: String,
    quit: bool,
}

pub fn run(args: TuiArgs) -> Result<()> {
    let api = Api::from_args(args.api_url.as_deref())?;
    let mut app = App {
        api,
        drafts: Vec::new(),
        selected: 0,
        view: View::List,
        status: String::new(),
        quit: false,
    };
    app.refresh();

    let mut terminal = ratatui::init();
    let result = loop {
        if let Err(error) = terminal.draw(|frame| draw(frame, &mut app)) {
            break Err(error.into());
        }
        if app.quit {
            break Ok(());
        }
        match event::poll(std::time::Duration::from_millis(250)) {
            Ok(true) => match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                    if key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        break Ok(());
                    }
                    app.handle_key(key.code);
                }
                Ok(_) => {}
                Err(error) => break Err(error.into()),
            },
            Ok(false) => {}
            Err(error) => break Err(error.into()),
        }
    };
    ratatui::restore();
    result
}

impl App {
    fn refresh(&mut self) {
        match self.api.drafts() {
            Ok(drafts) => {
                self.drafts = drafts;
                if self.selected >= self.drafts.len() {
                    self.selected = self.drafts.len().saturating_sub(1);
                }
                self.status = format!(
                    "{} draft{} · {}",
                    self.drafts.len(),
                    if self.drafts.len() == 1 { "" } else { "s" },
                    self.api.base_url
                );
            }
            Err(error) => self.status = format!("Error: {error}"),
        }
    }

    fn current_draft(&self) -> Option<&DraftSummary> {
        self.drafts.get(self.selected)
    }

    fn handle_key(&mut self, code: KeyCode) {
        match &mut self.view {
            View::List => self.handle_list_key(code),
            View::Versions { detail, selected } => match code {
                KeyCode::Char('q') | KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') => {
                    self.view = View::List;
                }
                KeyCode::Char('j') | KeyCode::Down => {
                    if *selected + 1 < detail.versions.len() {
                        *selected += 1;
                    }
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    *selected = selected.saturating_sub(1);
                }
                KeyCode::Char('o') | KeyCode::Enter => {
                    if let Some(version) = detail.versions.get(*selected) {
                        let url =
                            format!("{}/v/{}", detail.draft.public_url, version.version_number);
                        self.status = match open::that(&url) {
                            Ok(()) => format!("Opened {url}"),
                            Err(error) => format!("Error opening browser: {error}"),
                        };
                    }
                }
                _ => {}
            },
            View::ConfirmDelete { purge } => match code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    let purge = *purge;
                    self.view = View::List;
                    if let Some(draft) = self.current_draft() {
                        let draft_id = draft.draft_id.clone();
                        self.status = match self.api.delete_draft(&draft_id, purge) {
                            Ok(()) if purge => format!("Purged {draft_id}"),
                            Ok(()) => format!("Deleted {draft_id}"),
                            Err(error) => format!("Error: {error}"),
                        };
                        self.refresh();
                    }
                }
                _ => {
                    self.view = View::List;
                    self.status = "Delete cancelled.".to_string();
                }
            },
        }
    }

    fn handle_list_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('j') | KeyCode::Down => {
                if self.selected + 1 < self.drafts.len() {
                    self.selected += 1;
                }
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Char('g') | KeyCode::Home => self.selected = 0,
            KeyCode::Char('G') | KeyCode::End => {
                self.selected = self.drafts.len().saturating_sub(1);
            }
            KeyCode::Char('r') => {
                self.refresh();
                self.status = format!("Refreshed. {}", self.status);
            }
            KeyCode::Char('o') => {
                if let Some(draft) = self.current_draft() {
                    let url = draft.public_url.clone();
                    self.status = match open::that(&url) {
                        Ok(()) => format!("Opened {url}"),
                        Err(error) => format!("Error opening browser: {error}"),
                    };
                }
            }
            KeyCode::Char('y') => {
                if let Some(draft) = self.current_draft() {
                    self.status = format!("Raw: {}", draft.raw_url);
                }
            }
            KeyCode::Char('d') => {
                if self.current_draft().is_some() {
                    self.view = View::ConfirmDelete { purge: false };
                }
            }
            KeyCode::Char('D') => {
                if self.current_draft().is_some() {
                    self.view = View::ConfirmDelete { purge: true };
                }
            }
            KeyCode::Enter | KeyCode::Char('l') | KeyCode::Right => {
                if let Some(draft) = self.current_draft() {
                    let draft_id = draft.draft_id.clone();
                    match self.api.draft(&draft_id) {
                        Ok(detail) => {
                            self.view = View::Versions {
                                detail: Box::new(detail),
                                selected: 0,
                            }
                        }
                        Err(error) => self.status = format!("Error: {error}"),
                    }
                }
            }
            _ => {}
        }
    }
}

fn draw(frame: &mut Frame, app: &mut App) {
    let [header_area, main_area, footer_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .areas(frame.area());

    let header = Line::from(vec![
        Span::styled(" keryx ", Style::default().fg(Color::Black).bg(Color::Cyan)),
        Span::raw(" "),
        Span::styled(&app.api.base_url, Style::default().fg(Color::DarkGray)),
    ]);
    frame.render_widget(Paragraph::new(header), header_area);

    match &app.view {
        View::Versions { detail, selected } => draw_versions(frame, main_area, detail, *selected),
        _ => draw_list(frame, main_area, app),
    }

    let hints = match app.view {
        View::List | View::ConfirmDelete { .. } => {
            "j/k move · enter versions · o open · y raw url · d delete · D purge · r refresh · q quit"
        }
        View::Versions { .. } => "j/k move · o/enter open version · esc back",
    };
    let footer = Paragraph::new(vec![
        Line::from(Span::styled(hints, Style::default().fg(Color::DarkGray))),
        Line::from(Span::styled(
            app.status.as_str(),
            Style::default().fg(Color::Yellow),
        )),
    ]);
    frame.render_widget(footer, footer_area);

    if let View::ConfirmDelete { purge } = app.view {
        draw_confirm(frame, app, purge);
    }
}

fn draw_list(frame: &mut Frame, area: Rect, app: &mut App) {
    let items: Vec<ListItem> = if app.drafts.is_empty() {
        vec![ListItem::new(
            "No drafts yet. Publish one with: keryx upload <file>",
        )]
    } else {
        app.drafts
            .iter()
            .map(|draft| {
                let repo = match (&draft.repo_org, &draft.repo_name) {
                    (Some(org), Some(name)) => format!("{org}/{name}"),
                    _ => "no repo".to_string(),
                };
                let version = draft
                    .latest_version_number
                    .map(|n| format!("v{n}"))
                    .unwrap_or_else(|| "-".to_string());
                let mut meta = format!(
                    "  {repo} · {version} · {} version{} · updated {} · {}",
                    draft.version_count,
                    if draft.version_count == 1 { "" } else { "s" },
                    time_ago(&draft.updated_at),
                    draft.draft_id
                );
                if draft.disabled {
                    meta.push_str(" · DISABLED");
                }
                ListItem::new(vec![
                    Line::from(Span::styled(
                        draft.title.clone(),
                        Style::default().add_modifier(Modifier::BOLD),
                    )),
                    Line::from(Span::styled(meta, Style::default().fg(Color::DarkGray))),
                ])
            })
            .collect()
    };

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(" drafts "))
        .highlight_style(Style::default().bg(Color::Rgb(40, 44, 52)).fg(Color::Cyan));

    let mut state = ListState::default();
    if !app.drafts.is_empty() {
        state.select(Some(app.selected));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_versions(frame: &mut Frame, area: Rect, detail: &DraftDetail, selected: usize) {
    let items: Vec<ListItem> = detail
        .versions
        .iter()
        .map(|version| {
            let git = match (&version.git_branch, &version.git_commit_sha) {
                (Some(branch), Some(sha)) => {
                    format!("{branch}@{}", &sha[..sha.len().min(8)])
                }
                (Some(branch), None) => branch.clone(),
                _ => "no git info".to_string(),
            };
            let dirty = match version.git_dirty {
                Some(true) => " (dirty)",
                _ => "",
            };
            let line = format!(
                "v{} · {} · {}{} · {} bytes",
                version.version_number,
                time_ago(&version.created_at),
                git,
                dirty,
                version.file_size
            );
            ListItem::new(line)
        })
        .collect();

    let title = format!(" {} — versions ", detail.draft.title);
    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(title))
        .highlight_style(Style::default().bg(Color::Rgb(40, 44, 52)).fg(Color::Cyan));

    let mut state = ListState::default();
    if !detail.versions.is_empty() {
        state.select(Some(selected));
    }
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_confirm(frame: &mut Frame, app: &App, purge: bool) {
    let Some(draft) = app.current_draft() else {
        return;
    };
    let area = centered_rect(50, 6, frame.area());
    let text = if purge {
        format!(
            "PURGE \"{}\"?\nAll {} version{} and files are removed permanently. No undo.\n\ny = purge · any other key = cancel",
            draft.title,
            draft.version_count,
            if draft.version_count == 1 { "" } else { "s" }
        )
    } else {
        format!(
            "Delete \"{}\"?\n\ny = delete · any other key = cancel",
            draft.title
        )
    };
    let popup = Paragraph::new(text).wrap(Wrap { trim: true }).block(
        Block::default()
            .borders(Borders::ALL)
            .title(if purge {
                " confirm PURGE "
            } else {
                " confirm delete "
            })
            .style(Style::default().fg(Color::Red)),
    );
    frame.render_widget(Clear, area);
    frame.render_widget(popup, area);
}

fn centered_rect(width_percent: u16, height: u16, area: Rect) -> Rect {
    let width = area.width * width_percent / 100;
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    Rect {
        x,
        y,
        width,
        height: height.min(area.height),
    }
}
