use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use engine::calibration::{self, Completion, Options};
use engine::config::Config;
use tokio::signal::unix::{SignalKind, signal};

const CONFIG_VARIABLE: &str = "LOCAL_IDE_CONFIG";
const USAGE: &str = "usage: local-ide --config <path>, or set LOCAL_IDE_CONFIG
       local-ide calibrate --config <path> [--model <id>]... [--output <path>] [--state <path>] [--fresh]";
const INCOMPLETE_EXIT: u8 = 2;
// The container is small, and the engine serves one user with one run at a time
const WORKER_THREADS: usize = 2;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::INFO)
        .init();
    let Some(command) = command(std::env::args_os().skip(1).collect()) else {
        eprintln!("{USAGE}");
        return ExitCode::FAILURE;
    };
    let path = match &command {
        Command::Serve(path) | Command::Calibrate(path, _) => path,
    };
    let config = match Config::load(path) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("local-ide: {error}");
            return ExitCode::FAILURE;
        }
    };
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(WORKER_THREADS)
        .enable_all()
        .build();
    let runtime = match runtime {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("local-ide: cannot start the async runtime: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = match command {
        Command::Serve(_) => runtime.block_on(start(config)).map(|()| ExitCode::SUCCESS),
        Command::Calibrate(_, options) => runtime
            .block_on(calibration::calibrate(
                config,
                options,
                &mut std::io::stdout(),
            ))
            .map(|completion| match completion {
                Completion::Complete => ExitCode::SUCCESS,
                Completion::Incomplete => ExitCode::from(INCOMPLETE_EXIT),
            })
            .map_err(|error| error.to_string()),
    };
    result.unwrap_or_else(|message| {
        eprintln!("local-ide: {message}");
        ExitCode::FAILURE
    })
}

enum Command {
    Serve(PathBuf),
    Calibrate(PathBuf, Options),
}

fn command(arguments: Vec<OsString>) -> Option<Command> {
    let mut arguments = arguments.into_iter();
    match arguments.next() {
        Some(flag) if flag == "--config" => {
            arguments.next().map(|path| Command::Serve(path.into()))
        }
        Some(name) if name == "calibrate" => calibrate_command(arguments),
        Some(_) => None,
        None => std::env::var_os(CONFIG_VARIABLE).map(|path| Command::Serve(PathBuf::from(path))),
    }
}

fn calibrate_command(mut arguments: impl Iterator<Item = OsString>) -> Option<Command> {
    let mut config = None;
    let mut options = Options::default();
    while let Some(flag) = arguments.next() {
        match flag.to_str()? {
            "--config" if config.is_none() => config = Some(PathBuf::from(arguments.next()?)),
            "--model" => options.models.push(arguments.next()?.into_string().ok()?),
            "--output" if options.output.is_none() => {
                options.output = Some(PathBuf::from(arguments.next()?));
            }
            "--state" if options.state.is_none() => {
                options.state = Some(PathBuf::from(arguments.next()?));
            }
            "--fresh" if !options.fresh => options.fresh = true,
            _ => return None,
        }
    }
    Some(Command::Calibrate(config?, options))
}

async fn start(config: Config) -> Result<(), String> {
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(|error| format!("cannot listen on {}: {error}", config.listen))?;
    let mut terminate = signal(SignalKind::terminate()).map_err(|error| error.to_string())?;
    let mut interrupt = signal(SignalKind::interrupt()).map_err(|error| error.to_string())?;
    let shutdown = async move {
        tokio::select! {
            _ = terminate.recv() => {}
            _ = interrupt.recv() => {}
        }
        tracing::info!("stop signal received");
    };
    tracing::info!(listen = %config.listen, "local-ide listening");
    engine::serve(config, listener, shutdown)
        .await
        .map_err(|error| error.to_string())
}
