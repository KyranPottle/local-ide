use std::path::{Path, PathBuf};
use std::time::Duration;

use axum::http::StatusCode;
use engine::calibration::{CalibrationError, Completion, Options, calibrate};
use protocol::{ModelProfile, ToolCodec};
use serde_json::json;

use crate::support::*;

const ANSWER_PATH: &str = "src/answer.ts";

struct CalibrationStand {
    mock: MockInference,
    root: PathBuf,
}

impl Drop for CalibrationStand {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl CalibrationStand {
    async fn start() -> Self {
        Self {
            mock: MockInference::start(&SERVER_MODELS).await,
            root: temporary_directory(),
        }
    }

    fn config_text(&self, listen: &str) -> String {
        config_text(
            &self.root,
            listen,
            &self.mock.base_url,
            &Settings::default(),
        )
    }

    fn state_path(&self) -> PathBuf {
        self.root.join("var/calibration-state.json")
    }

    async fn calibrate(&self, options: Options) -> (Result<Completion, CalibrationError>, String) {
        self.calibrate_on("127.0.0.1:0", options).await
    }

    async fn calibrate_on(
        &self,
        listen: &str,
        options: Options,
    ) -> (Result<Completion, CalibrationError>, String) {
        let config = parse_config(&self.config_text(listen));
        let mut report = Vec::new();
        let result = calibrate(config, options, &mut report).await;
        (result, String::from_utf8(report).unwrap())
    }

    fn push_load_and_counts(&self) {
        self.mock
            .push(Script::Stream(usage_reply("ready", 12, 41.5)));
        self.mock.push(Script::Stream(usage_reply("x", 120, 41.5)));
        self.mock.push(Script::Stream(usage_reply("x", 296, 41.5)));
    }

    fn push_native_reads(&self) {
        for index in 0..3 {
            self.mock.push(Script::Stream(native_call_reply(
                &format!("call_probe_{index}"),
                "read",
                json!({ "path": "README.md" }),
            )));
        }
    }

    fn push_native_failures(&self) {
        for _ in 0..3 {
            self.mock.push(Script::Status(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("tools need --jinja, key {API_KEY}"),
            ));
        }
    }

    fn push_text_reads(&self) {
        for _ in 0..3 {
            self.mock
                .push_text(&text_call("read", json!({ "path": "README.md" })));
        }
    }

    fn push_native_tasks(&self) {
        for _ in 0..3 {
            self.mock.push(Script::Stream(native_call_reply(
                "call_read",
                "read",
                json!({ "path": ANSWER_PATH }),
            )));
            self.mock.push(Script::Stream(native_call_reply(
                "call_edit",
                "edit",
                json!({ "path": ANSWER_PATH, "old": "41", "new": "42" }),
            )));
            self.mock.push_text("Done.");
        }
    }

    fn push_text_tasks(&self, old: &str) {
        for _ in 0..3 {
            self.mock
                .push_text(&text_call("read", json!({ "path": ANSWER_PATH })));
            self.mock.push_text(&text_call(
                "edit",
                json!({ "path": ANSWER_PATH, "old": old, "new": "42" }),
            ));
            self.mock.push_text("Done.");
        }
    }

    fn push_unfit_model(&self) {
        self.push_load_and_counts();
        self.push_native_failures();
        for _ in 0..3 {
            self.mock.push_text("I cannot read files.");
        }
    }
}

fn only(models: &[&str]) -> Options {
    Options {
        models: models.iter().map(|id| (*id).to_owned()).collect(),
        ..Options::default()
    }
}

// The load time varies between runs, so the comparison leaves it out
fn without_load_line(report: &str) -> String {
    report
        .lines()
        .filter(|line| !line.starts_with("  load seconds: "))
        .map(|line| format!("{line}\n"))
        .collect()
}

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[tokio::test]
async fn native_and_text_pass_proposes_native() {
    let stand = CalibrationStand::start().await;
    stand.push_load_and_counts();
    stand.push_native_reads();
    stand.push_native_tasks();
    stand.push_text_reads();
    stand.push_text_tasks("41");
    let output = stand.root.join("proposed.toml");

    let (result, report) = stand
        .calibrate(Options {
            output: Some(output.clone()),
            ..only(&["alpha"])
        })
        .await;

    assert_eq!(result.unwrap(), Completion::Complete);
    assert_eq!(
        without_load_line(&report),
        format!(
            "model alpha
  tokens per second: 41.5
  characters per token: 3.75, profile 3.5
  native tool call: 3 of 3
  text tool call: 3 of 3
  native edit task: read 3 of 3, edit 3 of 3, finish 3 of 3
  text edit task: read 3 of 3, edit 3 of 3, finish 3 of 3
  proposed tool_codec: native
proposed profiles: {}
",
            output.display()
        )
    );
    let requests = stand.mock.requests();
    assert_eq!(requests.len(), 27);
    assert_eq!(stand.mock.models_requested(), vec!["alpha"; 27]);
    assert_eq!(stand.mock.most_in_flight(), 1);
    assert_eq!(requests[1]["max_tokens"], 1);
    assert_eq!(requests[3]["max_tokens"], 1000);
    assert_eq!(requests[3]["tools"][0]["function"]["name"], "read");
    assert_eq!(
        last_message_content(&requests[3]),
        "<workspace name=\"calibration\">\nsrc/\nREADME.md\n</workspace>\nRead the file README.md."
    );
    assert!(requests[15].get("tools").is_none());
    assert!(all_content(&requests[15]).contains("<tool_call>"));
    assert_eq!(
        requests[7]["messages"].as_array().unwrap().last().unwrap(),
        &json!({
            "role": "tool",
            "tool_call_id": "call_read",
            "content": "1|export const answer = 41"
        })
    );
}

#[tokio::test]
async fn proposed_profiles_parse_with_the_engine_configuration_parser() {
    let stand = CalibrationStand::start().await;
    stand.push_load_and_counts();
    stand.push_native_reads();
    stand.push_native_tasks();
    stand.push_text_reads();
    stand.push_text_tasks("41");
    let output = stand.root.join("proposed.toml");

    let (result, _) = stand
        .calibrate(Options {
            output: Some(output.clone()),
            ..only(&["alpha"])
        })
        .await;

    assert_eq!(result.unwrap(), Completion::Complete);
    let base = stand.config_text("127.0.0.1:0");
    let settings = &base[..base.find("[[model]]").unwrap()];
    let config = parse_config(&format!("{settings}{}", read(&output)));
    assert_eq!(
        config.models,
        [ModelProfile {
            id: "alpha".to_owned(),
            label: "Alpha".to_owned(),
            role: "General model".to_owned(),
            context_budget: 16000,
            reply_reserve: 1000,
            tool_codec: ToolCodec::Native,
            temperature: None,
            top_p: None,
            delegate_target: true,
            chars_per_token: 3.75,
        }]
    );
}

#[tokio::test]
async fn native_http_error_proposes_text_and_records_the_redacted_error() {
    let stand = CalibrationStand::start().await;
    stand.push_load_and_counts();
    stand.push_native_failures();
    stand.push_text_reads();
    stand.push_text_tasks("41");
    let output = stand.root.join("proposed.toml");

    let (result, report) = stand
        .calibrate(Options {
            output: Some(output.clone()),
            ..only(&["alpha"])
        })
        .await;

    assert_eq!(result.unwrap(), Completion::Complete);
    assert_eq!(
        without_load_line(&report),
        format!(
            "model alpha
  tokens per second: 41.5
  characters per token: 3.75, profile 3.5
  native tool call: 0 of 3
  text tool call: 3 of 3
  native edit task: not run
  text edit task: read 3 of 3, edit 3 of 3, finish 3 of 3
  proposed tool_codec: text
  failure native tool call: inference server returned HTTP 500: tools need --jinja, key [redacted]
proposed profiles: {}
",
            output.display()
        )
    );
    assert_eq!(stand.mock.requests().len(), 18);
    for text in [report, read(&output), read(&stand.state_path())] {
        assert!(!text.contains(API_KEY));
    }
}

#[tokio::test]
async fn a_model_that_passes_no_tool_probe_is_not_fit() {
    let stand = CalibrationStand::start().await;
    stand.push_unfit_model();

    let (result, report) = stand.calibrate(only(&["alpha"])).await;

    assert_eq!(result.unwrap(), Completion::Complete);
    assert_eq!(
        without_load_line(&report),
        "model alpha
  tokens per second: 41.5
  characters per token: 3.75, profile 3.5
  native tool call: 0 of 3
  text tool call: 0 of 3
  native edit task: not run
  text edit task: not run
  proposed tool_codec: none, the model is not fit for agent mode, text kept
  failure native tool call: inference server returned HTTP 500: tools need --jinja, key [redacted]
  failure text tool call: no tool call, reply text: \"I cannot read files.\"
"
    );
    assert_eq!(stand.mock.requests().len(), 9);
}

#[tokio::test]
async fn a_text_call_that_does_not_parse_reports_the_raw_reply() {
    let stand = CalibrationStand::start().await;
    stand.push_load_and_counts();
    stand.push_native_failures();
    for _ in 0..3 {
        stand.mock
            .push_text("<tool_call>\n<function=read>\n<parameter=path>\nREADME.md\n</parameter>\n</function>\n</tool_call>");
    }

    let (result, report) = stand.calibrate(only(&["alpha"])).await;

    assert_eq!(result.unwrap(), Completion::Complete);
    assert!(report.contains(
        "  failure text tool call: error: tool call is not valid JSON. Write <tool_call>{\"name\": ..., \"arguments\": {...}}</tool_call> reply text: \"<tool_call>\\n<function=read>"
    ));
}

#[tokio::test]
async fn an_edit_whose_old_text_does_not_match_fails_the_edit_step() {
    let stand = CalibrationStand::start().await;
    stand.push_load_and_counts();
    stand.push_native_failures();
    stand.push_text_reads();
    stand.push_text_tasks("40");

    let (result, report) = stand.calibrate(only(&["alpha"])).await;

    assert_eq!(result.unwrap(), Completion::Complete);
    assert!(report.contains("  text edit task: read 3 of 3, edit 0 of 3, finish 0 of 3\n"));
    assert!(
        report.contains("  failure text edit task: error: old text not found in src/answer.ts\n")
    );
    assert!(report.contains("  proposed tool_codec: text\n"));
    let requests = stand.mock.requests();
    assert_eq!(requests.len(), 18);
    assert_eq!(
        last_message_content(&requests[11]),
        "<tool_response>\nerror: old text not found in src/answer.ts\n</tool_response>"
    );
}

#[tokio::test]
async fn a_model_that_never_stops_ends_after_six_requests() {
    let stand = CalibrationStand::start().await;
    stand.push_load_and_counts();
    stand.push_native_failures();
    stand.push_text_reads();
    for _ in 0..18 {
        stand
            .mock
            .push_text(&text_call("glob", json!({ "pattern": "src/*" })));
    }
    stand
        .mock
        .push_text("an extra reply that the task never requests");

    let (result, report) = stand.calibrate(only(&["alpha"])).await;

    assert_eq!(result.unwrap(), Completion::Complete);
    assert!(report.contains("  text edit task: read 0 of 3, edit 0 of 3, finish 0 of 3\n"));
    assert!(report.contains("  failure text edit task: no final answer after 6 model requests\n"));
    assert_eq!(stand.mock.requests().len(), 27);
}

#[tokio::test]
async fn no_usage_keeps_the_profile_value() {
    let stand = CalibrationStand::start().await;
    stand.mock.push_text("ready");
    stand.mock.push_text("x");
    stand.push_native_failures();
    for _ in 0..3 {
        stand.mock.push_text("I cannot read files.");
    }
    let output = stand.root.join("proposed.toml");

    let (result, report) = stand
        .calibrate(Options {
            output: Some(output.clone()),
            ..only(&["alpha"])
        })
        .await;

    assert_eq!(result.unwrap(), Completion::Complete);
    assert!(report.contains("  tokens per second: not reported\n"));
    assert!(report.contains("  characters per token: no usage reported, profile value 3.5 kept\n"));
    assert_eq!(stand.mock.requests().len(), 8);
    assert!(read(&output).contains("chars_per_token = 3.5\n"));
}

#[tokio::test]
async fn resume_skips_a_finished_model() {
    let stand = CalibrationStand::start().await;
    stand.push_unfit_model();
    let (first, _) = stand.calibrate(only(&["alpha"])).await;
    assert_eq!(first.unwrap(), Completion::Complete);
    stand.push_unfit_model();

    let (result, report) = stand.calibrate(only(&["alpha", "beta"])).await;

    assert_eq!(result.unwrap(), Completion::Complete);
    assert!(report.starts_with("model alpha: skipped, finished in the state file\nmodel beta\n"));
    assert_eq!(stand.mock.models_requested()[9..], vec!["beta"; 9]);
}

#[tokio::test]
async fn fresh_measures_a_finished_model_again() {
    let stand = CalibrationStand::start().await;
    stand.push_unfit_model();
    let (first, _) = stand.calibrate(only(&["alpha"])).await;
    assert_eq!(first.unwrap(), Completion::Complete);
    stand.push_unfit_model();

    let (result, report) = stand
        .calibrate(Options {
            fresh: true,
            ..only(&["alpha"])
        })
        .await;

    assert_eq!(result.unwrap(), Completion::Complete);
    assert!(report.starts_with("model alpha\n"));
    assert_eq!(stand.mock.models_requested(), vec!["alpha"; 18]);
}

#[tokio::test]
async fn a_state_file_of_another_probe_set_is_refused_unless_fresh() {
    let stand = CalibrationStand::start().await;
    std::fs::create_dir_all(stand.root.join("var")).unwrap();
    std::fs::write(stand.state_path(), r#"{"probe_set": 7, "models": {}}"#).unwrap();

    let (result, _) = stand.calibrate(only(&["alpha"])).await;

    assert_eq!(
        result.unwrap_err().to_string(),
        format!(
            "the state file {} holds results of probe set version 7, not 1. Use --fresh to replace it",
            stand.state_path().display()
        )
    );
    assert!(stand.mock.requests().is_empty());
    stand.push_unfit_model();
    let (fresh, _) = stand
        .calibrate(Options {
            fresh: true,
            ..only(&["alpha"])
        })
        .await;
    assert_eq!(fresh.unwrap(), Completion::Complete);
    assert!(read(&stand.state_path()).contains("\"probe_set\": 1"));
}

#[tokio::test]
async fn a_bound_listen_address_stops_before_any_model_request() {
    let stand = CalibrationStand::start().await;
    let service = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let listen = service.local_addr().unwrap().to_string();

    let (result, report) = stand.calibrate_on(&listen, only(&["alpha"])).await;

    let message = result.unwrap_err().to_string();
    assert!(
        message.starts_with(&format!(
            "cannot bind {listen}: the local-ide service runs and must be stopped first"
        )),
        "{message}"
    );
    assert_eq!(report, "");
    assert!(stand.mock.requests().is_empty());
}

#[tokio::test]
async fn an_unknown_model_id_is_refused() {
    let stand = CalibrationStand::start().await;

    let (result, _) = stand.calibrate(only(&["delta"])).await;

    assert_eq!(
        result.unwrap_err().to_string(),
        "model \"delta\" has no profile in the configuration"
    );
}

fn binary() -> tokio::process::Command {
    let mut command = tokio::process::Command::new(env!("CARGO_BIN_EXE_local-ide"));
    command.env("TEST_KEY", API_KEY).kill_on_drop(true);
    command
}

#[tokio::test]
async fn the_command_exits_with_2_for_a_model_missing_on_the_server() {
    let stand = CalibrationStand::start().await;
    let config = stand.root.join("config.toml");
    std::fs::write(&config, stand.config_text("127.0.0.1:0")).unwrap();
    let output = stand.root.join("proposed.toml");

    let result = binary()
        .arg("calibrate")
        .arg("--config")
        .arg(&config)
        .args(["--model", "gamma", "--output"])
        .arg(&output)
        .output()
        .await
        .unwrap();

    assert_eq!(result.status.code(), Some(2));
    assert_eq!(
        String::from_utf8(result.stdout.clone()).unwrap(),
        format!(
            "model gamma: missing on the inference server\nproposed profiles: {}\n",
            output.display()
        )
    );
    for text in [result.stdout, result.stderr] {
        assert!(!String::from_utf8(text).unwrap().contains(API_KEY));
    }
    assert_eq!(read(&output), "model = []\n");
    assert!(stand.mock.requests().is_empty());
}

#[tokio::test]
async fn the_server_still_starts_with_config_and_no_subcommand() {
    let stand = CalibrationStand::start().await;
    let address = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap();
    let config = stand.root.join("config.toml");
    std::fs::write(&config, stand.config_text(&address.to_string())).unwrap();
    let _server = binary().arg("--config").arg(&config).spawn().unwrap();
    let http = reqwest::Client::new();

    let mut body = String::new();
    for _ in 0..100 {
        if let Ok(response) = http
            .get(format!("http://{address}/api/health"))
            .send()
            .await
        {
            body = response.text().await.unwrap();
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    assert_eq!(body, "ok");
}
