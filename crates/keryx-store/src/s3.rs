//! The S3 operator: AWS and every S3-compatible store (RustFS, MinIO, Ceph
//! RGW, R2, B2). Credentials never come from Keryx flags; they resolve through
//! the standard AWS chain: environment, shared profile, SSO,
//! credential_process, web identity, ECS, IMDS.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use opendal_core::{HttpTransporter, OperationContext, Operator};
use opendal_http_transport_reqwest::ReqwestTransport;
use opendal_service_s3::S3;
use reqsign_aws_v4::{
    AssumeRoleWithWebIdentityCredentialProvider, Credential, DefaultCredentialProvider,
    ECSCredentialProvider, EnvCredentialProvider, IMDSv2CredentialProvider,
    ProcessCredentialProvider, ProfileCredentialProvider, SSOCredentialProvider,
};
use reqsign_core::{
    CommandExecute, Context as SigningContext, Env, OsEnv, ProvideCredential,
    ProvideCredentialChain,
};

use crate::S3Config;

/// A black-holed endpoint fails fast instead of hanging startup or an upload.
const CONNECT_TIMEOUT: Duration = Duration::from_millis(3100);

/// Per-read inactivity deadline. Not a total-request timeout: a large draft on
/// a slow link is legitimate, a stalled socket is not.
const READ_INACTIVITY_TIMEOUT: Duration = Duration::from_secs(30);

pub(crate) fn describe(config: &S3Config) -> String {
    let prefix = config.prefix.trim_matches('/');
    if prefix.is_empty() {
        format!("s3://{}", config.bucket)
    } else {
        format!("s3://{}/{prefix}", config.bucket)
    }
}

pub(crate) fn create_s3_operator(config: &S3Config) -> Result<Operator> {
    // reqwest is built with rustls-no-provider. main() installs ring already;
    // repeating it here keeps library and test callers safe. Idempotent.
    let _ = rustls::crypto::ring::default_provider().install_default();

    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_INACTIVITY_TIMEOUT)
        .user_agent(concat!("keryx/", env!("CARGO_PKG_VERSION")))
        .build()
        .context("building S3 HTTP client")?;
    let context = OperationContext::new()
        .with_http_transport(HttpTransporter::new(ReqwestTransport::new(client)));

    let mut builder = S3::default()
        .bucket(&config.bucket)
        .region(&config.region)
        // The prefix is the operator root, so stored keys are identical on
        // every backend.
        .root(&format!("/{}", config.prefix.trim_matches('/')))
        // Transport integrity without requiring a provider to implement the
        // newer x-amz-checksum-* headers. Content-MD5 works on AWS and the
        // common S3-compatible stores.
        .checksum_algorithm("md5");
    let endpoint = config
        .endpoint
        .clone()
        .or_else(|| std::env::var("AWS_ENDPOINT_URL_S3").ok())
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    if let Some(endpoint) = endpoint {
        builder = builder.endpoint(&endpoint);
    }
    builder = builder.credential_provider_chain(ProvideCredentialChain::new().push(
        KeryxCredentialProvider::new(config.profile.clone(), &config.region),
    ));

    // No retry layer, deliberately: a failed request surfaces to the caller.
    Ok(Operator::new(builder)
        .context("building OpenDAL S3 operator")?
        .with_context(context))
}

/// Override only `AWS_PROFILE`, preserving every other process environment
/// value and the platform home-directory lookup.
#[derive(Debug, Clone)]
struct ProfileSelectingEnv<E> {
    inner: E,
    profile: String,
}

impl<E: Env> Env for ProfileSelectingEnv<E> {
    fn var(&self, key: &str) -> Option<String> {
        if key == "AWS_PROFILE" {
            Some(self.profile.clone())
        } else {
            self.inner.var(key)
        }
    }

    fn vars(&self) -> HashMap<String, String> {
        let mut vars = self.inner.vars();
        vars.insert("AWS_PROFILE".to_string(), self.profile.clone());
        vars
    }

    fn home_dir(&self) -> Option<PathBuf> {
        self.inner.home_dir()
    }
}

/// OpenDAL accepts a custom credential chain but exposes neither the selected
/// profile nor a command executor on its S3 builder. Wrapping reqsign's
/// default provider keeps `--s3-profile` and `credential_process` working
/// without mutating the process environment.
#[derive(Debug)]
struct KeryxCredentialProvider {
    inner: DefaultCredentialProvider,
    profile: Option<String>,
}

impl KeryxCredentialProvider {
    fn new(profile: Option<String>, region: &str) -> Self {
        // The AWS SDK's broad precedence: environment credentials first, then
        // the selected profile's providers, then workload identity and roles.
        let chain = ProvideCredentialChain::new()
            .push(EnvCredentialProvider::new())
            .push(ProfileCredentialProvider::default())
            .push(SSOCredentialProvider::default())
            .push(ProcessCredentialProvider::default())
            .push(
                AssumeRoleWithWebIdentityCredentialProvider::new().with_region(region.to_string()),
            )
            .push(ECSCredentialProvider::default())
            .push(IMDSv2CredentialProvider::default());
        Self {
            inner: DefaultCredentialProvider::with_chain(chain),
            profile,
        }
    }
}

impl ProvideCredential for KeryxCredentialProvider {
    type Credential = Credential;

    async fn provide_credential(
        &self,
        context: &SigningContext,
    ) -> reqsign_core::Result<Option<Self::Credential>> {
        let context = context.clone().with_command_execute(CredentialCommand {
            profile: self.profile.clone(),
        });
        if let Some(profile) = &self.profile {
            let context = context.with_env(ProfileSelectingEnv {
                inner: OsEnv,
                profile: profile.clone(),
            });
            self.inner.provide_credential(&context).await
        } else {
            self.inner.provide_credential(&context).await
        }
    }
}

/// Runs a profile's `credential_process` directly, never through a shell.
#[derive(Debug, Clone, Default)]
struct CredentialCommand {
    /// Profile to hand the child, when one was selected explicitly.
    profile: Option<String>,
}

impl CommandExecute for CredentialCommand {
    async fn command_execute(
        &self,
        program: &str,
        args: &[&str],
    ) -> reqsign_core::Result<reqsign_core::CommandOutput> {
        let (program, args) = relex_credential_command(program, args)
            .map_err(|error| reqsign_core::Error::config_invalid(format!("{error:#}")))?;

        // ProfileSelectingEnv only redirects reqsign's in-process reads. The
        // helper is a separate process, so it gets the profile on its own
        // environment; this process's environment is never mutated.
        let mut command = std::process::Command::new(&program);
        command.args(&args);
        if let Some(profile) = &self.profile {
            command.env("AWS_PROFILE", profile);
        }
        let output = tokio::task::spawn_blocking(move || command.output())
            .await
            .map_err(|error| {
                reqsign_core::Error::unexpected("credential_process task failed").with_source(error)
            })?
            .map_err(|error| {
                reqsign_core::Error::unexpected(format!("failed to execute command '{program}'"))
                    .with_source(error)
            })?;

        Ok(reqsign_core::CommandOutput {
            status: output.status.code().unwrap_or(-1),
            stdout: output.stdout,
            stderr: output.stderr,
        })
    }
}

/// reqsign splits the configured command on whitespace with no quote handling,
/// so `credential_process = "/opt/my helper" --role "a b"` arrives with the
/// quote characters still inside the tokens. Rejoin and re-lex it the way the
/// AWS SDKs do, without ever handing the string to a shell.
fn relex_credential_command(program: &str, args: &[&str]) -> Result<(String, Vec<String>)> {
    let mut command = program.to_string();
    for arg in args {
        command.push(' ');
        command.push_str(arg);
    }
    let tokens = shlex_split(&command).context("credential_process has unbalanced quotes")?;
    let mut tokens = tokens.into_iter();
    let program = tokens
        .next()
        .context("credential_process resolved to an empty command")?;
    Ok((program, tokens.collect()))
}

/// Minimal POSIX-style lexer: single quotes are literal, double quotes group
/// while honoring `\` escapes, and unquoted `\` escapes the next character.
/// No expansion of any kind. Returns `None` on unterminated quotes.
fn shlex_split(input: &str) -> Option<Vec<String>> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut has_token = false;
    let mut chars = input.chars();

    while let Some(c) = chars.next() {
        match c {
            c if c.is_whitespace() => {
                if has_token {
                    tokens.push(std::mem::take(&mut current));
                    has_token = false;
                }
            }
            '\'' => {
                has_token = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(c) => current.push(c),
                        None => return None,
                    }
                }
            }
            '"' => {
                has_token = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            // Only these are special inside double quotes.
                            Some(escaped @ ('"' | '\\' | '$' | '`')) => current.push(escaped),
                            Some(other) => {
                                current.push('\\');
                                current.push(other);
                            }
                            None => return None,
                        },
                        Some(c) => current.push(c),
                        None => return None,
                    }
                }
            }
            '\\' => {
                has_token = true;
                // Windows uses backslash as a path separator, so
                // `C:\tools\creds.exe` must survive intact.
                if cfg!(windows) {
                    current.push('\\');
                } else {
                    current.push(chars.next()?);
                }
            }
            c => {
                has_token = true;
                current.push(c);
            }
        }
    }
    if has_token {
        tokens.push(current);
    }
    Some(tokens)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(prefix: &str) -> S3Config {
        S3Config {
            bucket: "keryx-plans".to_string(),
            region: "us-east-1".to_string(),
            endpoint: Some("http://127.0.0.1:9".to_string()),
            prefix: prefix.to_string(),
            profile: Some("keryx".to_string()),
        }
    }

    #[test]
    fn operator_builds_with_profile_prefix_and_custom_endpoint() {
        let operator = create_s3_operator(&config("prod")).unwrap();
        assert_eq!(operator.info().root(), "/prod/");
        assert_eq!(describe(&config("/prod/")), "s3://keryx-plans/prod");
        assert_eq!(describe(&config("")), "s3://keryx-plans");
    }

    #[test]
    fn credential_command_relexing_restores_quoted_grouping() {
        let (program, args) =
            relex_credential_command("\"/opt/my", &["helper\"", "--role", "\"a", "b\""]).unwrap();
        assert_eq!(program, "/opt/my helper");
        assert_eq!(args, ["--role", "a b"]);
        assert!(relex_credential_command("\"/opt/unbalanced", &[]).is_err());
    }
}
