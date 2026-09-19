//! Layering the config file beneath clap. The file's values become clap's
//! defaults before parsing, so clap itself resolves
//! flag > environment variable > config.toml > built-in default, every
//! existing environment variable keeps its name, and `--help` shows what is
//! in effect.

use std::ffi::OsString;
use std::path::PathBuf;

use clap::Command;
use keryx_config::{FileConfig, CONFIG_ENV};

/// `--config <path>` or `--config=<path>` from the raw arguments, else
/// `KERYX_CONFIG`. Needed before clap parses, because the file it names
/// supplies clap's defaults.
pub fn explicit_path(args: &[OsString]) -> Option<PathBuf> {
    let mut args = args.iter().skip(1);
    while let Some(arg) = args.next() {
        let Some(arg) = arg.to_str() else { continue };
        if arg == "--" {
            break;
        }
        if arg == "--config" {
            return args.next().map(PathBuf::from);
        }
        if let Some(path) = arg.strip_prefix("--config=") {
            return Some(PathBuf::from(path));
        }
    }
    std::env::var_os(CONFIG_ENV)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

/// Give every argument the config file speaks for its value as the default.
pub fn apply_file_defaults(command: Command, config: &FileConfig) -> Command {
    let shared = |config: &FileConfig| {
        let mut defaults = config.database_arg_defaults();
        defaults.extend(config.s3_arg_defaults());
        defaults.extend(config.data_dir_arg_default());
        defaults
    };

    let mut serve = shared(config);
    serve.extend(config.serve_arg_defaults());
    serve.extend(config.storage_kind_arg_default());
    let migrate = shared(config);
    let mut gc = shared(config);
    gc.extend(config.storage_kind_arg_default());

    let command = command
        .mut_subcommand("serve", |serve_command| with_defaults(serve_command, serve))
        .mut_subcommand("storage", |storage| {
            storage
                .mut_subcommand("migrate", |command| with_defaults(command, migrate))
                .mut_subcommand("gc", |command| with_defaults(command, gc))
        });
    #[cfg(feature = "share")]
    let command = command.mut_subcommand("share", |share| {
        with_defaults(share, config.share_arg_defaults())
    });
    command
}

/// Arguments whose config value must never be echoed by `--help`.
const SECRET_ARGS: [&str; 2] = ["api_key", "database_url"];

fn with_defaults(mut command: Command, defaults: Vec<(&'static str, String)>) -> Command {
    for (id, value) in defaults {
        // clap wants 'static defaults. The config is loaded once and lives
        // for the process, so leaking a few short strings is the honest cost.
        let value: &'static str = Box::leak(value.into_boxed_str());
        command = command.mut_arg(id, |arg| {
            // A value from the file satisfies a required flag (`share --to`).
            arg.default_value(value)
                .required(false)
                .hide_default_value(SECRET_ARGS.contains(&id))
        });
    }
    command
}
