//! Manifest and report models for the generic Host Link v1 provider.

use crate::json::Json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Only this schema is understood; unknown versions degrade to nothing.
pub const SCHEMA: &str = "mc.host-link/v1";

/// One command from `commands.*`, used verbatim.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CommandSpec {
    pub exe: PathBuf,
    pub args: Vec<String>,
}

impl CommandSpec {
    fn from_json(value: &Json) -> Option<Self> {
        let exe = value.get("exe")?.as_str()?;
        if exe.is_empty() {
            return None;
        }
        // `args` may be absent; an empty command line is still valid.
        let args = match value.get("args") {
            Some(Json::Arr(items)) => items
                .iter()
                .filter_map(Json::as_str)
                .map(str::to_owned)
                .collect(),
            _ => Vec::new(),
        };
        Some(Self {
            exe: PathBuf::from(exe),
            args,
        })
    }
}

/// Pure file channel paths from `endpoints.files`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileEndpoints {
    pub link: Option<PathBuf>,
    pub probe: Option<PathBuf>,
    pub last_result: Option<PathBuf>,
}

/// A validated manifest. `id` is the only identity key.
#[derive(Clone, Debug, PartialEq)]
pub struct HostLink {
    pub id: String,
    pub name: String,
    pub exe: PathBuf,
    pub size: Option<u64>,
    pub modified_unix: Option<u64>,
    pub sha256: Option<String>,
    pub commands: HashMap<String, CommandSpec>,
    pub files: FileEndpoints,
    pub discovered_at: PathBuf,
    /// Raw `discovery` block, kept for diagnostics only.
    pub discovery: Option<Json>,
}

/// Parse result: either a usable link or a protocol error code.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseFailure {
    Unreadable,
    SchemaUnsupported,
    NoIdentity,
    NoExecutable,
    NoFingerprint,
}

impl ParseFailure {
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Unreadable => "LINK_UNREADABLE",
            Self::SchemaUnsupported => "LINK_SCHEMA_UNSUPPORTED",
            Self::NoIdentity => "LINK_UNREADABLE",
            Self::NoExecutable => "LINK_UNREADABLE",
            Self::NoFingerprint => "LINK_UNREADABLE",
        }
    }
}

/// Validate a manifest document against the v1 contract.
pub fn parse_manifest(text: &str, source: &Path) -> Result<HostLink, ParseFailure> {
    let json = crate::json::parse(text).map_err(|_| ParseFailure::Unreadable)?;
    let schema = json.get("schema").and_then(Json::as_str).unwrap_or("");
    if schema != SCHEMA {
        return Err(ParseFailure::SchemaUnsupported);
    }
    let app = json.get("app").ok_or(ParseFailure::NoIdentity)?;
    let id = app
        .get("id")
        .and_then(Json::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(ParseFailure::NoIdentity)?;
    let name = app
        .get("name")
        .and_then(Json::as_str)
        .unwrap_or(id)
        .to_owned();
    let executable = json.get("executable").ok_or(ParseFailure::NoExecutable)?;
    let path = executable
        .get("path")
        .and_then(Json::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(ParseFailure::NoExecutable)?;
    let size = executable
        .get("size")
        .and_then(Json::as_f64)
        .map(|v| v as u64);
    let modified_unix = executable
        .get("modified_unix")
        .and_then(Json::as_f64)
        .map(|v| v as u64);
    let sha256 = executable
        .get("sha256")
        .and_then(Json::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned);
    // At least one fingerprint is mandatory (protocol section 7).
    if size.is_none() && modified_unix.is_none() && sha256.is_none() {
        return Err(ParseFailure::NoFingerprint);
    }

    let mut commands = HashMap::new();
    if let Some(Json::Obj(items)) = json.get("commands") {
        for (key, value) in items {
            if let Some(spec) = CommandSpec::from_json(value) {
                commands.insert(key.clone(), spec);
            }
        }
    }
    let files = json
        .get("endpoints")
        .and_then(|e| e.get("files"))
        .map(|f| FileEndpoints {
            link: f.get("link").and_then(Json::as_str).map(PathBuf::from),
            probe: f.get("probe").and_then(Json::as_str).map(PathBuf::from),
            last_result: f
                .get("last_result")
                .and_then(Json::as_str)
                .map(PathBuf::from),
        })
        .unwrap_or_default();

    Ok(HostLink {
        id: id.to_owned(),
        name,
        exe: PathBuf::from(path),
        size,
        modified_unix,
        sha256,
        commands,
        files,
        discovered_at: source.to_path_buf(),
        discovery: json.get("discovery").cloned(),
    })
}

/// Error from `probe`, with the provider-side repair steps.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProbeError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepairStep {
    pub command: String,
    pub reason: String,
}

/// `mc.host-probe/v1` report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProbeReport {
    pub ok: bool,
    /// When the mirrored report was produced, if the provider published it.
    pub checked_unix: Option<u64>,
    pub error: Option<ProbeError>,
    pub repair: Vec<RepairStep>,
    pub ui_running: bool,
    /// Where the JSON came from: stdout or a mirrored file.
    pub source: String,
}

impl ProbeReport {
    /// Parse a probe document; a hidden UI (`ui.running: false`) is not an error.
    pub fn parse(text: &str, source: &str) -> Option<Self> {
        let json = crate::json::parse(text).ok()?;
        let schema = json.get("schema").and_then(Json::as_str).unwrap_or("");
        // The last-result mirror wraps the report as `mc.host-result/v1`.
        let body = match schema {
            "mc.host-probe/v1" => &json,
            "mc.host-result/v1" => json.get("result")?,
            _ => return None,
        };
        let error = body.get("error").map(|e| ProbeError {
            code: e
                .get("code")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_owned(),
            message: e
                .get("message")
                .and_then(Json::as_str)
                .unwrap_or("")
                .to_owned(),
        });
        let repair = match body.get("repair").and_then(Json::as_arr) {
            Some(items) => items
                .iter()
                .filter_map(|step| {
                    Some(RepairStep {
                        command: step.get("command")?.as_str()?.to_owned(),
                        reason: step
                            .get("reason")
                            .and_then(Json::as_str)
                            .unwrap_or("")
                            .to_owned(),
                    })
                })
                .collect(),
            None => Vec::new(),
        };
        let ui_running = body
            .get("checks")
            .and_then(Json::as_arr)
            .and_then(|checks| {
                checks
                    .iter()
                    .find(|c| c.get("name").and_then(Json::as_str) == Some("ui.running"))
            })
            .and_then(|c| c.get("ok"))
            .and_then(Json::as_bool)
            .unwrap_or(false);

        Some(Self {
            ok: body.get("ok").and_then(Json::as_bool).unwrap_or(false),
            checked_unix: body
                .get("checked_unix")
                .and_then(Json::as_f64)
                .map(|value| value as u64),
            error,
            repair,
            ui_running,
            source: source.to_owned(),
        })
    }

    /// Codes that invalidate the cached manifest (protocol section 7).
    #[must_use]
    pub fn needs_reread(&self) -> bool {
        matches!(
            self.error.as_ref().map(|e| e.code.as_str()),
            Some("EXE_CHANGED" | "LINK_STALE" | "EXE_MISSING")
        )
    }
}
