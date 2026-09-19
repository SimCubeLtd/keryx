mod cli;
mod config;
#[cfg(feature = "share")]
mod share;
mod tui;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "keryx",
    version,
    about = "Self-hosted static HTML draft publishing for agents — server, CLI, and TUI."
)]
struct Cli {
    /// Config file to read instead of ~/.config/keryx/config.toml. Flags
    /// override environment variables, which override the config file
    #[arg(long, global = true, env = "KERYX_CONFIG", value_name = "PATH")]
    config: Option<std::path::PathBuf>,

    #[command(subcommand)]
    command: Command,
}

/// Parse the command line with the config file's values as clap's defaults,
/// so flag > environment variable > config.toml > built-in default.
fn parse_cli() -> Cli {
    let args: Vec<std::ffi::OsString> = std::env::args_os().collect();
    let loaded = match keryx_config::init(config::explicit_path(&args).as_deref()) {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("{error:#}");
            std::process::exit(2);
        }
    };
    let command = config::apply_file_defaults(Cli::command(), &loaded.config);
    let matches = command.get_matches_from(args);
    Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit())
}

#[derive(Subcommand)]
enum Command {
    /// Run the keryx server
    Serve(keryx_server::ServeArgs),
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
    /// Share a draft version as an OCI artifact in any registry
    #[cfg(feature = "share")]
    Share(share::ShareArgs),
    /// Pull a shared draft version into Keryx, or to a file with --output
    #[cfg(feature = "share")]
    Pull(share::PullArgs),
    /// Show what a shared reference contains without downloading the document
    #[cfg(feature = "share")]
    Inspect(share::InspectArgs),
    /// Offline blob store maintenance: migrate between stores, collect orphans
    Storage {
        #[command(subcommand)]
        command: cli::StorageCommand,
    },
}

fn main() {
    // reqwest is built with rustls-no-provider, so an HTTPS request made before
    // this install panics. Process-wide and done once, here, because the CLI
    // makes HTTPS requests too, not only the server.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let cli = parse_cli();
    let result = match cli.command {
        Command::Serve(args) => keryx_server::run(args),
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
        #[cfg(feature = "share")]
        Command::Share(args) => share::share(args),
        #[cfg(feature = "share")]
        Command::Pull(args) => share::pull(args),
        #[cfg(feature = "share")]
        Command::Inspect(args) => share::inspect(args),
        Command::Storage { command } => cli::storage(command),
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

    /// A config file that speaks for every argument it can, written to a
    /// temp file and loaded the way the binary loads it.
    fn full_config() -> keryx_config::FileConfig {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            r#"
            [client]
            share_to = "ghcr.io/acme/plans"
            [server]
            host = "0.0.0.0"
            port = 9000
            data_dir = "/var/lib/keryx"
            public_base_url = "https://plans.example.com"
            api_key = "file-secret"
            max_html_bytes = 1048576
            allow_font_links = true
            allow_safe_handlers = true
            allow_inline_scripts = true
            push_contact = "mailto:ops@example.com"
            [database]
            path = "/var/lib/keryx/keryx.db"
            url = "postgres://keryx:db-secret@db/keryx"
            pool_size = 8
            no_backup = true
            [storage]
            kind = "s3"
            [storage.s3]
            bucket = "keryx-plans"
            region = "eu-west-2"
            endpoint = "http://rustfs:9000"
            prefix = "prod"
            profile = "keryx"
            "#,
        )
        .unwrap();
        keryx_config::load_file(&path).unwrap()
    }

    fn parse_with(config: &keryx_config::FileConfig, args: &[&str]) -> Cli {
        let command = config::apply_file_defaults(Cli::command(), config);
        let matches = command.try_get_matches_from(args).unwrap();
        Cli::from_arg_matches(&matches).unwrap()
    }

    fn serve_args(cli: Cli) -> keryx_server::ServeArgs {
        match cli.command {
            Command::Serve(args) => args,
            _ => panic!("expected serve"),
        }
    }

    #[test]
    fn the_config_file_speaks_for_every_argument_it_names() {
        // mut_arg panics on an id clap does not know, so a full config also
        // proves every key maps onto a real argument.
        let args = serve_args(parse_with(&full_config(), &["keryx", "serve"]));
        assert_eq!(args.host, "0.0.0.0");
        assert_eq!(
            args.data_dir.as_deref(),
            Some(std::path::Path::new("/var/lib/keryx"))
        );
        assert_eq!(args.api_key.as_deref(), Some("file-secret"));
        assert_eq!(args.max_html_bytes, 1_048_576);
        assert!(args.allow_font_links && args.allow_safe_handlers && args.allow_inline_scripts);
        assert_eq!(args.storage, keryx_server::StorageKind::S3);
        assert_eq!(args.s3.s3_bucket.as_deref(), Some("keryx-plans"));
        assert_eq!(args.s3.s3_region, "eu-west-2");
        assert_eq!(args.s3.s3_prefix, "prod");
        assert_eq!(args.database.db_pool_size, Some(8));
        assert!(args.database.no_backup);
        assert!(args.database.database_url.unwrap().contains("db-secret"));

        // The storage commands read the same database and S3 sections.
        parse_with(&full_config(), &["keryx", "storage", "gc"]);
        parse_with(
            &full_config(),
            &[
                "keryx", "storage", "migrate", "--from", "disk", "--to", "s3",
            ],
        );
    }

    #[test]
    fn flags_override_environment_variables_which_override_the_config_file() {
        let config = full_config();
        assert_eq!(
            serve_args(parse_with(&config, &["keryx", "serve"])).port,
            9000
        );

        // No other test reads KERYX_PORT, so setting it here is safe.
        std::env::set_var("KERYX_PORT", "9100");
        assert_eq!(
            serve_args(parse_with(&config, &["keryx", "serve"])).port,
            9100
        );
        let flagged = parse_with(&config, &["keryx", "serve", "--port", "9200"]);
        assert_eq!(serve_args(flagged).port, 9200);
        std::env::remove_var("KERYX_PORT");

        // With no config file the built-in default still stands.
        let empty = keryx_config::FileConfig::default();
        assert_eq!(
            serve_args(parse_with(&empty, &["keryx", "serve"])).port,
            7812
        );
    }

    #[test]
    fn help_never_prints_a_secret_from_the_config_file() {
        let mut command = config::apply_file_defaults(Cli::command(), &full_config());
        let serve = command.find_subcommand_mut("serve").unwrap();
        let help = serve.render_long_help().to_string();
        assert!(help.contains("9000"), "ordinary defaults are shown");
        assert!(!help.contains("file-secret"), "{help}");
        assert!(!help.contains("db-secret"), "{help}");
    }

    #[cfg(feature = "share")]
    #[test]
    fn share_to_from_the_config_file_satisfies_the_required_flag() {
        let cli = parse_with(&full_config(), &["keryx", "share", "abc123def456"]);
        let Command::Share(args) = cli.command else {
            panic!("expected share")
        };
        assert_eq!(args.to, "ghcr.io/acme/plans");

        let empty = keryx_config::FileConfig::default();
        let command = config::apply_file_defaults(Cli::command(), &empty);
        assert!(command
            .try_get_matches_from(["keryx", "share", "abc123def456"])
            .is_err());
    }

    #[test]
    fn the_config_flag_is_found_before_clap_parses() {
        let args = |list: &[&str]| {
            list.iter()
                .map(std::ffi::OsString::from)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            config::explicit_path(&args(&["keryx", "--config", "/etc/keryx.toml", "serve"])),
            Some("/etc/keryx.toml".into())
        );
        assert_eq!(
            config::explicit_path(&args(&["keryx", "serve", "--config=/etc/keryx.toml"])),
            Some("/etc/keryx.toml".into())
        );
    }
}
