//! CLI-side state (~/.keryx) and a small blocking HTTP client for the
//! keryx API.

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::policy::PolicyOptions;
use crate::types::{DraftDetail, DraftSummary, UploadResponse};

pub const DEFAULT_API_URL: &str = "http://localhost:7812";

pub fn state_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".keryx")
}

fn config_path() -> PathBuf {
    state_dir().join("config.json")
}

fn credentials_path() -> PathBuf {
    state_dir().join("credentials.json")
}

fn drafts_path() -> PathBuf {
    state_dir().join("drafts.json")
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct CliConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_url: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Credentials {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DraftMappings {
    pub files: BTreeMap<String, DraftMapping>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftMapping {
    pub draft_id: String,
    pub public_url: String,
    pub raw_url: String,
    pub latest_version_number: i64,
    pub updated_at: String,
}

fn read_json<T: Default + for<'de> Deserialize<'de>>(path: &PathBuf) -> T {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

fn write_json<T: Serialize>(path: &PathBuf, value: &T) -> Result<()> {
    std::fs::create_dir_all(state_dir())?;
    let text = format!("{}\n", serde_json::to_string_pretty(value)?);
    std::fs::write(path, text)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub fn read_drafts() -> DraftMappings {
    read_json(&drafts_path())
}

pub fn write_drafts(drafts: &DraftMappings) -> Result<()> {
    write_json(&drafts_path(), drafts)
}

pub fn save_credentials(api_key: Option<&str>, api_url_override: Option<&str>) -> Result<()> {
    if let Some(url) = api_url_override {
        let mut config: CliConfig = read_json(&config_path());
        config.api_url = Some(url.trim_end_matches('/').to_string());
        write_json(&config_path(), &config)?;
    }
    write_json(
        &credentials_path(),
        &Credentials {
            api_key: api_key.map(str::to_string),
            updated_at: Some(crate::db::now()),
        },
    )
}

pub struct CliAuth {
    pub api_url: String,
    pub api_key: Option<String>,
}

/// Resolution order: flag > KERYX_API_URL > ~/.keryx/config.json >
/// default (localhost, since this is self-hosted).
pub fn read_auth(api_url_override: Option<&str>) -> CliAuth {
    let config: CliConfig = read_json(&config_path());
    let credentials: Credentials = read_json(&credentials_path());
    let api_url = api_url_override
        .map(str::to_string)
        .or_else(|| std::env::var("KERYX_API_URL").ok())
        .or(config.api_url)
        .unwrap_or_else(|| DEFAULT_API_URL.to_string())
        .trim_end_matches('/')
        .to_string();
    let api_key = std::env::var("KERYX_API_KEY").ok().or(credentials.api_key);
    CliAuth { api_url, api_key }
}

pub struct Api {
    pub base_url: String,
    api_key: Option<String>,
    http: reqwest::blocking::Client,
}

#[derive(Debug)]
pub struct PublishResult {
    pub draft_id: String,
    pub version_number: i64,
    pub public_url: String,
    pub raw_url: String,
    pub page_count: u32,
    pub output_path: PathBuf,
}

impl Api {
    pub fn new(auth: CliAuth) -> Result<Self> {
        Ok(Self {
            base_url: auth.api_url,
            api_key: auth.api_key,
            http: reqwest::blocking::Client::builder()
                .user_agent(format!("keryx/{}", env!("CARGO_PKG_VERSION")))
                .build()?,
        })
    }

    pub fn from_args(api_url_override: Option<&str>) -> Result<Self> {
        Self::new(read_auth(api_url_override))
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::blocking::RequestBuilder {
        let mut builder = self
            .http
            .request(method, format!("{}{}", self.base_url, path));
        if let Some(key) = &self.api_key {
            builder = builder.bearer_auth(key);
        }
        builder
    }

    fn json_body(response: reqwest::blocking::Response) -> Result<Value> {
        let status = response.status();
        let body: Value = response
            .json()
            .with_context(|| format!("server returned a non-JSON response (HTTP {status})"))?;
        if !status.is_success() {
            let message = body
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("Request failed.");
            let details = body
                .get("errors")
                .and_then(Value::as_array)
                .map(|errors| {
                    errors
                        .iter()
                        .filter_map(Value::as_str)
                        .map(|e| format!("\n- {e}"))
                        .collect::<String>()
                })
                .unwrap_or_default();
            bail!("{message}{details}");
        }
        Ok(body)
    }

    pub fn me(&self) -> Result<()> {
        let response = self.request(reqwest::Method::GET, "/api/me").send()?;
        Self::json_body(response).map(|_| ())
    }

    /// The server's effective upload policy, so local validation matches what
    /// the server will accept. Falls back to the defaults when the server is
    /// unreachable or predates the field — the upload itself still decides.
    pub fn policy(&self) -> PolicyOptions {
        self.request(reqwest::Method::GET, "/api/me")
            .send()
            .ok()
            .and_then(|response| response.json::<Value>().ok())
            .and_then(|body| serde_json::from_value(body.get("policy")?.clone()).ok())
            .unwrap_or_default()
    }

    pub fn upload(&self, payload: &Value) -> Result<UploadResponse> {
        let response = self
            .request(reqwest::Method::POST, "/api/uploads")
            .json(payload)
            .send()?;
        let body = Self::json_body(response)?;
        Ok(serde_json::from_value(body)?)
    }

    pub fn drafts(&self) -> Result<Vec<DraftSummary>> {
        let response = self.request(reqwest::Method::GET, "/api/drafts").send()?;
        let body = Self::json_body(response)?;
        let drafts = body
            .get("drafts")
            .cloned()
            .ok_or_else(|| anyhow!("response is missing \"drafts\""))?;
        Ok(serde_json::from_value(drafts)?)
    }

    pub fn draft(&self, draft_id: &str) -> Result<DraftDetail> {
        let response = self
            .request(reqwest::Method::GET, &format!("/api/drafts/{draft_id}"))
            .send()?;
        let body = Self::json_body(response)?;
        let draft = body
            .get("draft")
            .cloned()
            .ok_or_else(|| anyhow!("response is missing \"draft\""))?;
        Ok(serde_json::from_value(draft)?)
    }

    pub fn delete_draft(&self, draft_id: &str, purge: bool) -> Result<()> {
        let path = if purge {
            format!("/api/drafts/{draft_id}?purge=true")
        } else {
            format!("/api/drafts/{draft_id}")
        };
        let response = self.request(reqwest::Method::DELETE, &path).send()?;
        Self::json_body(response).map(|_| ())
    }

    /// Hard-delete every soft-deleted draft; returns how many were removed.
    pub fn purge_deleted(&self) -> Result<i64> {
        let response = self.request(reqwest::Method::POST, "/api/purge").send()?;
        let body = Self::json_body(response)?;
        Ok(body
            .get("purgedDrafts")
            .and_then(Value::as_i64)
            .unwrap_or(0))
    }

    pub fn raw_html(&self, draft_id: &str, version: Option<i64>) -> Result<String> {
        let path = match version {
            Some(n) => format!("/d/{draft_id}/v/{n}/raw"),
            None => format!("/d/{draft_id}/raw"),
        };
        let response = self.request(reqwest::Method::GET, &path).send()?;
        let status = response.status();
        if !status.is_success() {
            bail!("draft fetch failed (HTTP {status})");
        }
        Ok(response.text()?)
    }

    /// Request a PDF for one stored Keryx version and atomically materialize
    /// it at `output`. Existing files are never replaced.
    pub fn publish_to_path(
        &self,
        draft_id: &str,
        version: Option<i64>,
        output: &Path,
    ) -> Result<PublishResult> {
        if version.is_some_and(|version| version < 1) {
            bail!("version must be at least 1");
        }
        validate_output_path(output)?;

        let mut request =
            self.request(reqwest::Method::GET, &format!("/api/drafts/{draft_id}/pdf"));
        if let Some(version) = version {
            request = request.query(&[("version", version)]);
        }
        let mut response = request.send()?;
        let status = response.status();
        if !status.is_success() {
            Self::json_body(response)?;
            bail!("PDF request failed (HTTP {status})");
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default();
        if !content_type.starts_with("application/pdf") {
            bail!("server returned {content_type:?} instead of a PDF");
        }

        let response_draft_id = required_header(&response, "x-keryx-draft-id")?;
        let version_number = required_header(&response, "x-keryx-draft-version")?
            .parse::<i64>()
            .context("server returned an invalid Keryx version header")?;
        let public_url = required_header(&response, "x-keryx-public-url")?;
        let raw_url = required_header(&response, "x-keryx-raw-url")?;
        let page_count = required_header(&response, "x-keryx-pdf-pages")?
            .parse::<u32>()
            .context("server returned an invalid PDF page count")?;

        persist_pdf(&mut response, output)?;
        Ok(PublishResult {
            draft_id: response_draft_id,
            version_number,
            public_url,
            raw_url,
            page_count,
            output_path: output.to_path_buf(),
        })
    }

    pub fn public_url(&self, draft_id: &str) -> String {
        format!("{}/d/{draft_id}", self.base_url)
    }
}

fn required_header(response: &reqwest::blocking::Response, name: &str) -> Result<String> {
    response
        .headers()
        .get(name)
        .with_context(|| format!("server response is missing {name}"))?
        .to_str()
        .with_context(|| format!("server returned an invalid {name} header"))
        .map(str::to_string)
}

fn validate_output_path(output: &Path) -> Result<&Path> {
    if output.file_name().is_none() {
        bail!("output must name a PDF file");
    }
    if output.exists() {
        bail!("refusing to replace existing file: {}", output.display());
    }
    let parent = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    if !parent.is_dir() {
        bail!("output directory does not exist: {}", parent.display());
    }
    Ok(parent)
}

fn persist_pdf(reader: &mut impl Read, output: &Path) -> Result<()> {
    let parent = validate_output_path(output)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("creating a temporary PDF in {}", parent.display()))?;
    std::io::copy(reader, temporary.as_file_mut())
        .with_context(|| format!("writing temporary PDF for {}", output.display()))?;
    temporary.as_file_mut().flush()?;
    temporary.as_file_mut().seek(SeekFrom::Start(0))?;
    let mut magic = [0_u8; 5];
    temporary
        .as_file_mut()
        .read_exact(&mut magic)
        .context("server returned a truncated PDF")?;
    if &magic != b"%PDF-" {
        bail!("server returned invalid PDF bytes");
    }
    temporary.as_file().sync_all()?;
    temporary
        .persist_noclobber(output)
        .map_err(|error| error.error)
        .with_context(|| format!("publishing PDF to {}", output.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_pdf_without_clobbering_an_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("report.pdf");
        persist_pdf(&mut std::io::Cursor::new(b"%PDF-test"), &output).unwrap();
        assert_eq!(std::fs::read(&output).unwrap(), b"%PDF-test");

        let error = persist_pdf(&mut std::io::Cursor::new(b"%PDF-replacement"), &output)
            .unwrap_err()
            .to_string();
        assert!(error.contains("refusing to replace"));
        assert_eq!(std::fs::read(&output).unwrap(), b"%PDF-test");
    }

    #[test]
    fn removes_temporary_output_when_pdf_is_invalid() {
        let directory = tempfile::tempdir().unwrap();
        let output = directory.path().join("report.pdf");
        assert!(persist_pdf(&mut std::io::Cursor::new(b"not-pdf"), &output).is_err());
        assert!(!output.exists());
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }
}
