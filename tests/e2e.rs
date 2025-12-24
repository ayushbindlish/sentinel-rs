use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use mockito::{Matcher, Server};
use serde_json::json;

fn command_with_mock(server: &Server) -> Command {
    let mut cmd = cargo_bin_cmd!("sentinel-rs");
    cmd.env("TG_BOT_TOKEN", "TEST_TOKEN")
        .env("TG_CHAT_ID", "123")
        .env("TG_API_BASE", server.url());
    cmd
}

#[test]
fn success_sends_start_and_finish_notifications() {
    let mut server = Server::new();
    let mock = server
        .mock("POST", "/botTEST_TOKEN/sendMessage")
        .match_body(Matcher::PartialJson(json!({"chat_id": "123"})))
        .expect(2)
        .create();

    let mut cmd = command_with_mock(&server);
    cmd.arg("--").arg("true");
    cmd.assert().success();
    mock.assert();
    drop(server);
}

#[test]
fn failure_propagates_exit_code_and_sends_notifications() {
    let mut server = Server::new();
    let mock = server
        .mock("POST", "/botTEST_TOKEN/sendMessage")
        .match_body(Matcher::PartialJson(json!({"chat_id": "123"})))
        .expect(2)
        .create();

    let mut cmd = command_with_mock(&server);
    cmd.arg("--").arg("exit").arg("101");
    cmd.assert().code(101);
    mock.assert();
    drop(server);
}

#[test]
fn help_does_not_require_env() {
    let mut cmd = cargo_bin_cmd!("sentinel-rs");
    cmd.arg("--help");
    cmd.assert()
        .success()
        .stderr(predicates::str::contains("Usage: sentinel-rs"));
}

#[test]
fn stdin_is_forwarded_to_child_command() {
    let mut server = Server::new();
    let mock = server
        .mock("POST", "/botTEST_TOKEN/sendMessage")
        .match_body(Matcher::PartialJson(json!({"chat_id": "123"})))
        .expect(2)
        .create();

    let mut cmd = command_with_mock(&server);
    cmd.arg("--").arg("cat");
    cmd.write_stdin("hello stdin\n");
    cmd.assert()
        .success()
        .stdout(predicates::str::contains("hello stdin"));
    mock.assert();
    drop(server);
}

#[test]
fn missing_args_exits_with_usage() {
    let mut cmd = cargo_bin_cmd!("sentinel-rs");
    cmd.assert()
        .code(2)
        .stderr(predicates::str::contains("Usage: sentinel-rs"));
}

#[test]
fn missing_command_after_double_dash_exits_2() {
    let mut cmd = cargo_bin_cmd!("sentinel-rs");
    cmd.arg("--");
    cmd.assert()
        .code(2)
        .stderr(predicates::str::contains("Missing command after --."));
}

#[test]
fn spawn_failure_exits_1() {
    let mut server = Server::new();
    let mock = server
        .mock("POST", "/botTEST_TOKEN/sendMessage")
        .match_body(Matcher::PartialJson(json!({"chat_id": "123"})))
        .expect(2)
        .create();

    let mut cmd = command_with_mock(&server);
    cmd.env("PATH", "");
    cmd.arg("--").arg("true");
    cmd.assert().code(1);
    mock.assert();
    drop(server);
}

#[test]
fn telegram_failure_does_not_change_exit_code() {
    let mut cmd = cargo_bin_cmd!("sentinel-rs");
    cmd.env("TG_BOT_TOKEN", "TEST_TOKEN")
        .env("TG_CHAT_ID", "123")
        .env("TG_API_BASE", "http://127.0.0.1:1")
        .arg("--")
        .arg("true");
    cmd.assert().success();
}
