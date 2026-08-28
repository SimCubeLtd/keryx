mod cli;
mod client;
mod db;
mod gitmeta;
mod ids;
mod notifications;
mod pdf;
mod policy;
mod realtime;
mod render;
mod server;
mod storage;
mod tui;
mod types;

use clap::{Parser, Subcommand};
use sha2::{Digest, Sha256};

#[derive(Parser)]
#[command(
    name = "keryx",
    version,
    about = "Self-hosted static HTML draft publishing for agents — server, CLI, and TUI."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Run the keryx server
    Serve(server::ServeArgs),
    /// Upload or update an HTML draft
    Upload(cli::UploadArgs),
    /// List published drafts
    List(cli::ListArgs),
    /// Print a draft's raw HTML to stdout
    Raw(cli::RawArgs),
    /// Publish an immutable draft version as a PDF
    Publish(cli::PublishArgs),
    /// Open a draft in the browser
    Open(cli::OpenArgs),
    /// Hide a draft from the dashboard until a wake time; links keep working
    Snooze(cli::SnoozeArgs),
    /// Wake a snoozed draft now
    Unsnooze(cli::DraftArgs),
    /// Stop serving a draft; every public link returns 404 until enabled
    Disable(cli::DisableArgs),
    /// Serve a disabled draft again
    Enable(cli::DraftArgs),
    /// Delete a draft (soft by default; --purge removes it permanently)
    Delete(cli::DeleteArgs),
    /// Permanently remove all soft-deleted drafts and their files
    Purge(cli::PurgeArgs),
    /// Manage CLI authentication
    Auth {
        #[command(subcommand)]
        command: cli::AuthCommand,
    },
    /// Browse drafts interactively
    Tui(tui::TuiArgs),
}

pub fn sha256_hex(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}

fn main() {
    let cli = Cli::parse();
    let result = match cli.command {
        Command::Serve(args) => server::run(args),
        Command::Upload(args) => cli::upload(args),
        Command::List(args) => cli::list(args),
        Command::Raw(args) => cli::raw(args),
        Command::Publish(args) => cli::publish(args),
        Command::Open(args) => cli::open(args),
        Command::Snooze(args) => cli::snooze(args),
        Command::Unsnooze(args) => cli::unsnooze(args),
        Command::Disable(args) => cli::disable(args),
        Command::Enable(args) => cli::enable(args),
        Command::Delete(args) => cli::delete(args),
        Command::Purge(args) => cli::purge(args),
        Command::Auth { command } => cli::auth(command),
        Command::Tui(args) => tui::run(args),
    };

    if let Err(error) = result {
        eprintln!("{error:#}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn publish_requires_named_id_and_output_flags() {
        assert!(Cli::try_parse_from(["keryx", "publish", "--id", "abc123def456"]).is_err());
        assert!(Cli::try_parse_from([
            "keryx",
            "publish",
            "--id",
            "abc123def456",
            "--output",
            "/tmp/report.pdf",
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "keryx",
            "publish",
            "--id",
            "abc123def456",
            "--version",
            "3",
            "--output",
            "/tmp/report.pdf",
        ])
        .is_ok());
    }

    #[test]
    fn snooze_requires_exactly_one_wake_time() {
        assert!(Cli::try_parse_from(["keryx", "snooze", "abc123def456"]).is_err());
        assert!(Cli::try_parse_from(["keryx", "snooze", "abc123def456", "--for", "2h"]).is_ok());
        assert!(Cli::try_parse_from([
            "keryx",
            "snooze",
            "abc123def456",
            "--until",
            "2026-08-28T08:00:00Z",
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "keryx",
            "snooze",
            "abc123def456",
            "--for",
            "2h",
            "--until",
            "2026-08-28T08:00:00Z",
        ])
        .is_err());
        assert!(Cli::try_parse_from(["keryx", "list", "--snoozed", "--include-snoozed"]).is_err());
    }
}
