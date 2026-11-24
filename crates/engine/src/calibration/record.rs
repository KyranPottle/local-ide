use std::collections::BTreeMap;
use std::io::{self, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use protocol::{ModelProfile, ToolCodec};
use serde::{Deserialize, Serialize};

use super::CalibrationError;

// Changes whenever a probe definition changes, so that old results are not mixed with new ones
pub(super) const PROBE_SET: u32 = 1;
pub(super) const RUNS: u32 = 3;
const DETAIL_CHARS: usize = 200;
const TEXT_PROBE_MINIMUM: u32 = 2;

// A detail can hold server or model text, and a control character could drive the terminal
fn cut(detail: &str) -> String {
    detail
        .chars()
        .take(DETAIL_CHARS)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum CharsPerToken {
    Measured { value: f32 },
    // The server sent no usage, so the profile value stays
    NotReported,
    NotIncreasing { short: u32, long: u32 },
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct ProbeTally {
    passes: u32,
    first_failure: Option<String>,
}

impl ProbeTally {
    pub fn add(&mut self, result: Result<(), String>) {
        match result {
            Ok(()) => self.passes += 1,
            Err(detail) => {
                self.first_failure.get_or_insert_with(|| cut(&detail));
            }
        }
    }

    pub const fn passes(&self) -> u32 {
        self.passes
    }
}

#[derive(Debug, Default)]
pub(super) struct TaskRun {
    pub reached_read: bool,
    pub correct_edit: bool,
    pub finished: bool,
    failure: Option<String>,
}

impl TaskRun {
    pub fn note_failure(&mut self, detail: String) {
        self.failure.get_or_insert_with(|| cut(&detail));
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum TaskTally {
    // The tool call probe of the codec passed in no run
    NotRun,
    Ran {
        read: u32,
        edit: u32,
        finish: u32,
        first_failure: Option<String>,
    },
}

impl TaskTally {
    pub fn of(runs: Vec<TaskRun>) -> Self {
        let count = |stage: fn(&TaskRun) -> bool| {
            u32::try_from(runs.iter().filter(|run| stage(run)).count()).unwrap_or(u32::MAX)
        };
        Self::Ran {
            read: count(|run| run.reached_read),
            edit: count(|run| run.correct_edit),
            finish: count(|run| run.finished),
            first_failure: runs
                .iter()
                .filter(|run| !(run.reached_read && run.correct_edit && run.finished))
                .find_map(|run| run.failure.clone()),
        }
    }

    fn score(&self) -> u32 {
        match self {
            Self::NotRun => 0,
            Self::Ran {
                read, edit, finish, ..
            } => read + edit + finish,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(super) struct ModelMeasurement {
    pub load_seconds: f64,
    pub tokens_per_second: Option<f32>,
    pub chars_per_token: CharsPerToken,
    pub native_probe: ProbeTally,
    pub text_probe: ProbeTally,
    pub native_task: TaskTally,
    pub text_task: TaskTally,
}

enum CodecProposal {
    Native,
    Text,
    NotFit,
}

impl ModelMeasurement {
    fn proposal(&self) -> CodecProposal {
        if self.native_probe.passes == RUNS && self.native_task.score() >= self.text_task.score() {
            return CodecProposal::Native;
        }
        if self.text_probe.passes >= TEXT_PROBE_MINIMUM {
            return CodecProposal::Text;
        }
        CodecProposal::NotFit
    }

    pub fn proposed_profile(&self, profile: &ModelProfile) -> ModelProfile {
        let tool_codec = match self.proposal() {
            CodecProposal::Native => ToolCodec::Native,
            CodecProposal::Text => ToolCodec::Text,
            CodecProposal::NotFit => profile.tool_codec,
        };
        let chars_per_token = match self.chars_per_token {
            CharsPerToken::Measured { value } => (value * 100.0).round() / 100.0,
            CharsPerToken::NotReported | CharsPerToken::NotIncreasing { .. } => {
                profile.chars_per_token
            }
        };
        ModelProfile {
            tool_codec,
            chars_per_token,
            ..profile.clone()
        }
    }

    pub fn write_report(&self, out: &mut impl Write, profile: &ModelProfile) -> io::Result<()> {
        writeln!(out, "model {}", profile.id)?;
        writeln!(out, "  load seconds: {:.2}", self.load_seconds)?;
        match self.tokens_per_second {
            Some(speed) => writeln!(out, "  tokens per second: {speed:.1}")?,
            None => writeln!(out, "  tokens per second: not reported")?,
        }
        match &self.chars_per_token {
            CharsPerToken::Measured { value } => writeln!(
                out,
                "  characters per token: {value:.2}, profile {}",
                profile.chars_per_token
            )?,
            CharsPerToken::NotReported => writeln!(
                out,
                "  characters per token: no usage reported, profile value {} kept",
                profile.chars_per_token
            )?,
            CharsPerToken::NotIncreasing { short, long } => writeln!(
                out,
                "  characters per token: prompt tokens {short} and {long} do not increase, profile value {} kept",
                profile.chars_per_token
            )?,
        }
        for (name, tally) in [
            ("native tool call", &self.native_probe),
            ("text tool call", &self.text_probe),
        ] {
            writeln!(out, "  {name}: {} of {RUNS}", tally.passes)?;
        }
        for (name, task) in [
            ("native edit task", &self.native_task),
            ("text edit task", &self.text_task),
        ] {
            match task {
                TaskTally::NotRun => writeln!(out, "  {name}: not run")?,
                TaskTally::Ran {
                    read, edit, finish, ..
                } => writeln!(
                    out,
                    "  {name}: read {read} of {RUNS}, edit {edit} of {RUNS}, finish {finish} of {RUNS}"
                )?,
            }
        }
        match self.proposal() {
            CodecProposal::Native => writeln!(out, "  proposed tool_codec: native")?,
            CodecProposal::Text => writeln!(out, "  proposed tool_codec: text")?,
            CodecProposal::NotFit => writeln!(
                out,
                "  proposed tool_codec: none, the model is not fit for agent mode, {} kept",
                codec_name(profile.tool_codec)
            )?,
        }
        let failures = [
            ("native tool call", self.native_probe.first_failure.as_ref()),
            ("text tool call", self.text_probe.first_failure.as_ref()),
            ("native edit task", task_failure(&self.native_task)),
            ("text edit task", task_failure(&self.text_task)),
        ];
        for (name, detail) in failures {
            if let Some(detail) = detail {
                writeln!(out, "  failure {name}: {detail}")?;
            }
        }
        Ok(())
    }
}

const fn codec_name(codec: ToolCodec) -> &'static str {
    match codec {
        ToolCodec::Native => "native",
        ToolCodec::Text => "text",
    }
}

fn task_failure(task: &TaskTally) -> Option<&String> {
    match task {
        TaskTally::NotRun => None,
        TaskTally::Ran { first_failure, .. } => first_failure.as_ref(),
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub(super) struct CalibrationState {
    probe_set: u32,
    pub models: BTreeMap<String, ModelMeasurement>,
}

#[derive(Deserialize)]
struct StateVersion {
    probe_set: u32,
}

impl CalibrationState {
    pub fn empty() -> Self {
        Self {
            probe_set: PROBE_SET,
            models: BTreeMap::new(),
        }
    }

    // An absent file gives an empty state
    pub fn load(path: &Path) -> Result<Self, CalibrationError> {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Self::empty()),
            Err(source) => {
                return Err(CalibrationError::StateRead {
                    path: path.to_owned(),
                    source,
                });
            }
        };
        let decode_error = |error: serde_json::Error| CalibrationError::StateDecode {
            path: path.to_owned(),
            reason: error.to_string(),
        };
        let version: StateVersion = serde_json::from_str(&text).map_err(decode_error)?;
        if version.probe_set != PROBE_SET {
            return Err(CalibrationError::StateVersion {
                path: path.to_owned(),
                found: version.probe_set,
            });
        }
        serde_json::from_str(&text).map_err(decode_error)
    }

    pub fn save(&self, path: &Path) -> Result<(), CalibrationError> {
        let text = serde_json::to_vec_pretty(self).map_err(io::Error::other);
        text.and_then(|text| write_private(path, &text))
            .map_err(|source| CalibrationError::Write {
                path: path.to_owned(),
                source,
            })
    }
}

#[derive(Serialize)]
struct ProposedProfiles<'a> {
    model: &'a [ModelProfile],
}

pub(super) fn write_proposal(
    path: &Path,
    profiles: &[ModelProfile],
) -> Result<(), CalibrationError> {
    let write_error = |source: io::Error| CalibrationError::Write {
        path: path.to_owned(),
        source,
    };
    let text = toml::to_string(&ProposedProfiles { model: profiles })
        .map_err(|error| write_error(io::Error::other(error)))?;
    write_private(path, text.as_bytes()).map_err(write_error)
}

// Writes a sibling file with mode 600 and renames it, so a reader never sees a partial file
fn write_private(path: &Path, contents: &[u8]) -> io::Result<()> {
    if let Some(directory) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(directory)?;
    }
    let mut partial = path.as_os_str().to_owned();
    partial.push(".partial");
    let partial = PathBuf::from(partial);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&partial)?;
    file.write_all(contents)?;
    file.sync_all()?;
    std::fs::rename(&partial, path)
}
