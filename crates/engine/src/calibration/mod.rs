mod exchange;
mod probes;
mod record;

use std::io::{self, Write};
use std::net::SocketAddr;
use std::path::PathBuf;

use inference::{InferenceClient, InferenceError};
use protocol::{ModelProfile, ToolCodec};

use crate::config::Config;
use exchange::ExchangeError;
use record::{CalibrationState, ModelMeasurement, ProbeTally, RUNS, TaskTally};

pub const STATE_FILE: &str = "calibration-state.json";

#[derive(Debug, Default)]
pub struct Options {
    // Configured model ids, empty for every configured profile
    pub models: Vec<String>,
    pub output: Option<PathBuf>,
    // None for STATE_FILE in the configured data_dir
    pub state: Option<PathBuf>,
    pub fresh: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Completion {
    Complete,
    Incomplete,
}

#[derive(Debug, thiserror::Error)]
pub enum CalibrationError {
    #[error("model {0:?} has no profile in the configuration")]
    UnknownModel(String),
    #[error(
        "cannot bind {listen}: the local-ide service runs and must be stopped first ({source})"
    )]
    ServiceRunning {
        listen: SocketAddr,
        source: io::Error,
    },
    #[error("cannot read the state file {path}: {source}")]
    StateRead { path: PathBuf, source: io::Error },
    #[error("the state file {path} does not decode: {reason}. Use --fresh to replace it")]
    StateDecode { path: PathBuf, reason: String },
    #[error(
        "the state file {path} holds results of probe set version {found}, not {expected}. Use --fresh to replace it",
        expected = record::PROBE_SET
    )]
    StateVersion { path: PathBuf, found: u32 },
    #[error("cannot write {path}: {source}")]
    Write { path: PathBuf, source: io::Error },
    #[error("cannot set up the inference client: {0}")]
    Inference(#[from] InferenceError),
    #[error("the inference server did not list its models: {0}")]
    ModelList(InferenceError),
    #[error("cannot write the report: {0}")]
    Report(#[from] io::Error),
}

enum ModelOutcome {
    Measured(ModelMeasurement),
    Interrupted { probe: &'static str, detail: String },
}

pub async fn calibrate(
    config: Config,
    options: Options,
    report: &mut impl Write,
) -> Result<Completion, CalibrationError> {
    if let Some(unknown) = options
        .models
        .iter()
        .find(|id| config.models.iter().all(|profile| profile.id != **id))
    {
        return Err(CalibrationError::UnknownModel(unknown.clone()));
    }
    let selected: Vec<ModelProfile> = config
        .models
        .into_iter()
        .filter(|profile| options.models.is_empty() || options.models.contains(&profile.id))
        .collect();
    // The bind proves that the service does not hold the GPU, and the listener drops at once
    std::net::TcpListener::bind(config.listen).map_err(|source| {
        CalibrationError::ServiceRunning {
            listen: config.listen,
            source,
        }
    })?;
    let state_path = options
        .state
        .unwrap_or_else(|| config.data_dir.join(STATE_FILE));
    let mut state = if options.fresh {
        CalibrationState::empty()
    } else {
        CalibrationState::load(&state_path)?
    };
    let client = InferenceClient::new(config.inference)?;
    let server_ids = client
        .model_ids()
        .await
        .map_err(CalibrationError::ModelList)?;
    let mut completion = Completion::Complete;
    for profile in &selected {
        if state.models.contains_key(&profile.id) {
            writeln!(
                report,
                "model {}: skipped, finished in the state file",
                profile.id
            )?;
            continue;
        }
        if !server_ids.contains(&profile.id) {
            writeln!(
                report,
                "model {}: missing on the inference server",
                profile.id
            )?;
            completion = Completion::Incomplete;
            continue;
        }
        tracing::info!(model = %profile.id, "calibration: model started");
        match measure(&client, profile).await {
            ModelOutcome::Measured(measurement) => {
                measurement.write_report(report, profile)?;
                state.models.insert(profile.id.clone(), measurement);
                tracing::info!(model = %profile.id, "calibration: model done");
                state.save(&state_path)?;
                tracing::info!(path = %state_path.display(), "calibration: state saved");
            }
            ModelOutcome::Interrupted { probe, detail } => {
                writeln!(
                    report,
                    "model {}: the {probe} probe could not run: {detail}",
                    profile.id
                )?;
                tracing::warn!(model = %profile.id, probe, "calibration: model interrupted");
                completion = Completion::Incomplete;
            }
        }
    }
    if let Some(output) = &options.output {
        let proposed: Vec<ModelProfile> = selected
            .iter()
            .filter_map(|profile| {
                let measurement = state.models.get(&profile.id)?;
                Some(measurement.proposed_profile(profile))
            })
            .collect();
        record::write_proposal(output, &proposed)?;
        writeln!(report, "proposed profiles: {}", output.display())?;
    }
    Ok(completion)
}

async fn measure(client: &InferenceClient, profile: &ModelProfile) -> ModelOutcome {
    let interrupted = |probe: &'static str, error: ExchangeError| ModelOutcome::Interrupted {
        probe,
        detail: error.into_detail(),
    };
    let load = match probes::measure_load(client, profile).await {
        Ok(load) => load,
        Err(error) => return interrupted("load", error),
    };
    let chars_per_token = match probes::measure_chars_per_token(client, profile).await {
        Ok(chars_per_token) => chars_per_token,
        Err(error) => return interrupted("characters per token", error),
    };
    let (native_probe, native_task) = match measure_codec(client, profile, ToolCodec::Native).await
    {
        Ok(results) => results,
        Err(outcome) => return outcome,
    };
    let (text_probe, text_task) = match measure_codec(client, profile, ToolCodec::Text).await {
        Ok(results) => results,
        Err(outcome) => return outcome,
    };
    ModelOutcome::Measured(ModelMeasurement {
        load_seconds: load.seconds,
        tokens_per_second: load.tokens_per_second,
        chars_per_token,
        native_probe,
        text_probe,
        native_task,
        text_task,
    })
}

async fn measure_codec(
    client: &InferenceClient,
    profile: &ModelProfile,
    codec: ToolCodec,
) -> Result<(ProbeTally, TaskTally), ModelOutcome> {
    let interrupted =
        |probe: &'static str| move |detail: String| ModelOutcome::Interrupted { probe, detail };
    let mut tally = ProbeTally::default();
    for _ in 0..RUNS {
        let result = probes::probe_tool_call(client, profile, codec)
            .await
            .map_err(interrupted("tool call"))?;
        tally.add(result);
    }
    if tally.passes() == 0 {
        return Ok((tally, TaskTally::NotRun));
    }
    let mut runs = Vec::new();
    for _ in 0..RUNS {
        let run = probes::run_edit_task(client, profile, codec)
            .await
            .map_err(interrupted("edit task"))?;
        runs.push(run);
    }
    Ok((tally, TaskTally::of(runs)))
}
