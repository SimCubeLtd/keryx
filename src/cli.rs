//! CLI subcommands: upload, publish, list, raw, open, delete, and auth.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use clap::{Args, Subcommand};
use serde_json::json;

use crate::client::{read_auth, save_credentials, Api, CliAuth, DraftMapping};
use crate::gitmeta;
use crate::policy::validate_html;

#[derive(Args, Debug)]
pub struct UploadArgs {
    /// HTML file path
    pub file: PathBuf,
    /// Update a specific draft
    #[arg(long)]
    pub draft: Option<String>,
    /// Always create a new draft
    #[arg(long)]
    pub new: bool,
    /// Set a short description for the draft
    #[arg(long)]
    pub description: Option<String>,
    /// Override the Keryx API base URL
    #[arg(long)]
    pub api_url: Option<String>,
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Print the raw JSON response
    #[arg(long)]
    pub json: bool,
    /// Override the Keryx API base URL
    #[arg(long)]
    pub api_url: Option<String>,
}

#[derive(Args, Debug)]
pub struct RawArgs {
    /// Draft id
    pub draft_id: String,
    /// Fetch a specific version
    #[arg(long, short = 'v')]
    pub version: Option<i64>,
    /// Override the Keryx API base URL
    #[arg(long)]
    pub api_url: Option<String>,
}

#[derive(Args, Debug)]
pub struct PublishArgs {
    /// Keryx draft id
    #[arg(long)]
    pub id: String,
    /// Publish a specific immutable version (latest when omitted)
    #[arg(long)]
    pub version: Option<i64>,
    /// Destination PDF path (must not already exist)
    #[arg(long)]
    pub output: PathBuf,
    /// Override the Keryx API base URL
    #[arg(long)]
    pub api_url: Option<String>,
}

#[derive(Args, Debug)]
pub struct OpenArgs {
    /// Draft id
    pub draft_id: String,
    /// Override the Keryx API base URL
    #[arg(long)]
    pub api_url: Option<String>,
}

#[derive(Args, Debug)]
pub struct DeleteArgs {
    /// Draft id
    pub draft_id: String,
    /// Permanently remove the draft, all versions, and their files
    #[arg(long)]
    pub purge: bool,
    /// Skip the confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Override the Keryx API base URL
    #[arg(long)]
    pub api_url: Option<String>,
}

#[derive(Args, Debug)]
pub struct PurgeArgs {
    /// Skip the confirmation prompt
    #[arg(long, short = 'y')]
    pub yes: bool,
    /// Override the Keryx API base URL
    #[arg(long)]
    pub api_url: Option<String>,
}

#[derive(Subcommand, Debug)]
pub enum AuthCommand {
    /// Save an API key (verified against the server first)
    Set {
        /// Keryx API key
        api_key: String,
        /// Override the Keryx API base URL
        #[arg(long)]
        api_url: Option<String>,
    },
    /// Remove the stored API key
    Clear,
}

pub fn upload(args: UploadArgs) -> Result<()> {
    let file = args
        .file
        .canonicalize()
        .with_context(|| format!("File does not exist: {}", args.file.display()))?;
    let html =
        std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;

    let api = Api::from_args(args.api_url.as_deref())?;

    let validation = validate_html(&html, &api.policy());
    if !validation.ok() {
        bail!(
            "HTML failed Keryx validation:\n- {}",
            validation.errors.join("\n- ")
        );
    }

    let mut drafts = crate::client::read_drafts();
    let file_key = file.to_string_lossy().to_string();
    let known_draft_id = drafts.files.get(&file_key).map(|m| m.draft_id.clone());
    let draft_id = if args.new {
        None
    } else {
        args.draft.clone().or(known_draft_id)
    };

    // Provenance describes the checkout the upload was invoked from. HTML
    // communication workflows deliberately keep their artifact under /tmp,
    // so discovering Git from the file's parent loses the repository context.
    let invocation_dir = std::env::current_dir().context("reading the current directory")?;
    let metadata = gitmeta::collect(&invocation_dir);
    let filename = file
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "draft.html".to_string());

    let payload = json!({
        "html": html,
        "filename": filename,
        "draftId": draft_id,
        "description": args.description,
        "metadata": metadata,
    });

    let updating = draft_id.is_some();
    let response = api.upload(&payload)?;

    drafts.files.insert(
        file_key,
        DraftMapping {
            draft_id: response.draft_id.clone(),
            public_url: response.public_url.clone(),
            raw_url: response.raw_url.clone(),
            latest_version_number: response.version_number,
            updated_at: crate::db::now(),
        },
    );
    crate::client::write_drafts(&drafts)?;

    println!(
        "{}",
        if updating {
            "Updated draft"
        } else {
            "Uploaded draft"
        }
    );
    println!("URL: {}", response.public_url);
    println!("Raw HTML: {}", response.raw_url);
    println!("Draft ID: {}", response.draft_id);
    println!("Version: {}", response.version_number);
    for warning in &response.warnings {
        eprintln!("Warning: {warning}");
    }
    Ok(())
}

pub fn list(args: ListArgs) -> Result<()> {
    let api = Api::from_args(args.api_url.as_deref())?;
    let drafts = api.drafts()?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&drafts)?);
        return Ok(());
    }

    if drafts.is_empty() {
        println!("No drafts yet. Publish one with: keryx upload <file>");
        return Ok(());
    }

    println!("Drafts ({})\n", drafts.len());
    for draft in &drafts {
        let repo = match (&draft.repo_org, &draft.repo_name) {
            (Some(org), Some(name)) => format!("{org}/{name}"),
            _ => "no repo".to_string(),
        };
        let version = draft
            .latest_version_number
            .map(|n| format!("v{n}"))
            .unwrap_or_else(|| "no versions".to_string());
        let count = format!(
            "{} version{}",
            draft.version_count,
            if draft.version_count == 1 { "" } else { "s" }
        );
        let disabled = if draft.disabled { " · disabled" } else { "" };

        println!("{}", draft.title);
        println!(
            "  {repo} · {version} · {count} · updated {}{disabled}",
            time_ago(&draft.updated_at)
        );
        println!("  {}", draft.public_url);
        if let Some(description) = &draft.description {
            println!("  {description}");
        }
        println!();
    }
    Ok(())
}

pub fn raw(args: RawArgs) -> Result<()> {
    let api = Api::from_args(args.api_url.as_deref())?;
    let html = api.raw_html(&args.draft_id, args.version)?;
    print!("{html}");
    Ok(())
}

pub fn publish(args: PublishArgs) -> Result<()> {
    let api = Api::from_args(args.api_url.as_deref())?;
    let result = api.publish_to_path(&args.id, args.version, &args.output)?;
    println!("Published PDF");
    println!("Output: {}", result.output_path.display());
    println!("Draft ID: {}", result.draft_id);
    println!("Version: {}", result.version_number);
    println!("Pages: {}", result.page_count);
    println!("Source: {}", result.public_url);
    println!("Raw HTML: {}", result.raw_url);
    Ok(())
}

pub fn open(args: OpenArgs) -> Result<()> {
    let api = Api::from_args(args.api_url.as_deref())?;
    let url = api.public_url(&args.draft_id);
    open::that(&url).with_context(|| format!("opening {url}"))?;
    println!("Opened {url}");
    Ok(())
}

pub fn delete(args: DeleteArgs) -> Result<()> {
    let api = Api::from_args(args.api_url.as_deref())?;

    if !args.yes {
        let prompt = if args.purge {
            format!(
                "PERMANENTLY delete draft {} — all versions and files, no undo? [y/N] ",
                args.draft_id
            )
        } else {
            format!("Delete draft {}? [y/N] ", args.draft_id)
        };
        if !confirm(&prompt)? {
            println!("Aborted.");
            return Ok(());
        }
    }

    api.delete_draft(&args.draft_id, args.purge)?;
    println!(
        "{} draft {}",
        if args.purge { "Purged" } else { "Deleted" },
        args.draft_id
    );
    Ok(())
}

pub fn purge(args: PurgeArgs) -> Result<()> {
    let api = Api::from_args(args.api_url.as_deref())?;

    if !args.yes && !confirm("Permanently remove all soft-deleted drafts and their files? [y/N] ")?
    {
        println!("Aborted.");
        return Ok(());
    }

    let count = api.purge_deleted()?;
    println!("Purged {count} draft{}.", if count == 1 { "" } else { "s" });
    Ok(())
}

fn confirm(prompt: &str) -> Result<bool> {
    print!("{prompt}");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
}

pub fn auth(command: AuthCommand) -> Result<()> {
    match command {
        AuthCommand::Set { api_key, api_url } => {
            let auth = read_auth(api_url.as_deref());
            let api = Api::new(CliAuth {
                api_url: auth.api_url,
                api_key: Some(api_key.clone()),
            })?;
            api.me()
                .map_err(|_| anyhow::anyhow!("That key was rejected. Nothing saved."))?;
            save_credentials(Some(&api_key), api_url.as_deref())?;
            println!("Keryx credentials saved.");
        }
        AuthCommand::Clear => {
            save_credentials(None, None)?;
            println!("Keryx credentials cleared.");
        }
    }
    Ok(())
}

pub fn time_ago(value: &str) -> String {
    let Ok(then) = DateTime::parse_from_rfc3339(value) else {
        return "unknown".to_string();
    };
    let seconds = (Utc::now() - then.with_timezone(&Utc)).num_seconds().max(0);

    const UNITS: &[(&str, i64)] = &[
        ("year", 31_536_000),
        ("month", 2_592_000),
        ("week", 604_800),
        ("day", 86_400),
        ("hour", 3_600),
        ("minute", 60),
    ];

    for (name, unit_seconds) in UNITS {
        let amount = seconds / unit_seconds;
        if amount >= 1 {
            return format!("{amount} {name}{} ago", if amount == 1 { "" } else { "s" });
        }
    }
    "just now".to_string()
}
