use chrono::{Local};
use hostname::get;
use log::info;
use reqwest::blocking::Client;
use serde_json::json;
use std::env;
use std::io::{Read, Write};
use std::process::{Command, Output, Stdio};

fn env_required(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("Environment variable {key} is required"))
}

fn format_message(ts: &str, host: &str, text: &str) -> String {
    format!("[{ts}] [{host}]\n{text}")
}

fn telegram_payload(chat_id: &str, body: &str) -> serde_json::Value {
    json!({
        "chat_id": chat_id,
        "text": body,
        "disable_web_page_preview": true,
    })
}

fn tg_send(text: &str) {
    let bot_token = env_required("TG_BOT_TOKEN");
    let chat_id = env_required("TG_CHAT_ID");
    let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
    let host = get().unwrap_or_default().to_string_lossy().to_string();
    let ts = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let body = format_message(&ts, &host, text);
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .expect("Failed to build HTTP client");
    let res = client
        .post(&url)
        .json(&telegram_payload(&chat_id, &body))
        .send();
    if let Err(e) = res {
        eprintln!("Failed to send Telegram message: {e}");
    }
}

fn read_stream<R: Read, W: Write>(mut reader: R, mut writer: W, tee: bool) -> Vec<u8> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = reader.read(&mut chunk).expect("Failed to read stream");
        if read == 0 {
            break;
        }
        if tee {
            writer
                .write_all(&chunk[..read])
                .expect("Failed to write stream");
            writer.flush().ok();
        }
        buf.extend_from_slice(&chunk[..read]);
    }
    buf
}

fn run_bash_with_tee(command: &str, tee: bool) -> Output {
    let mut child = Command::new("bash")
        .arg("-c")
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("Failed to execute command");

    let mut stdout = child.stdout.take().expect("Failed to capture stdout");
    let mut stderr = child.stderr.take().expect("Failed to capture stderr");

    let stdout_handle = std::thread::spawn(move || read_stream(stdout, std::io::stdout(), tee));
    let stderr_handle = std::thread::spawn(move || read_stream(stderr, std::io::stderr(), tee));

    let status = child.wait().expect("Failed to wait on child");
    let out_buf = stdout_handle.join().expect("Failed to join stdout thread");
    let err_buf = stderr_handle.join().expect("Failed to join stderr thread");

    Output {
        status,
        stdout: out_buf,
        stderr: err_buf,
    }
}

fn run_bash(command: &str) -> Output {
    run_bash_with_tee(command, true)
}

fn tail_bytes(buf: &[u8], max: usize) -> String {
    if buf.len() <= max {
        String::from_utf8_lossy(buf).into_owned()
    } else {
        let slice = &buf[buf.len() - max..];
        format!(
            "… (truncated, showing last {} bytes)\n{}",
            max,
            String::from_utf8_lossy(slice)
        )
    }
}

fn main() {
    env_logger::init();
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        eprintln!("Usage: sentinel-rs <any text to send on startup>");
        std::process::exit(2);
    }
    let command = args.join(" ");
    tg_send(&format!("Started\n{command}"));

    let output = run_bash(&command);

    match output.status.code() {
        Some(0) => {
            tg_send(&format!(
                "Finished successfully with exit code 0.\nStdout:\n{}\nStderr:\n{}",
                tail_bytes(&output.stdout, 1500),
                tail_bytes(&output.stderr, 1500)
            ));
            info!("Command finished successfully with exit code 0");
        }
        Some(code) => {
            tg_send(&format!(
                "Failed with exit code: {}.\nStdout:\n{}\nStderr:\n{}",
                code,
                tail_bytes(&output.stdout, 1500),
                tail_bytes(&output.stderr, 1500)
            ));
            info!(
                "Failed with exit code: {}. Stdout: {} Stderr: {}",
                code,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        None => {
            tg_send(&format!(
                "Process terminated by signal.\nStdout:\n{}\nStderr:\n{}",
                tail_bytes(&output.stdout, 1500),
                tail_bytes(&output.stderr, 1500)
            ));
            info!("Process terminated by signal.");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_required_missing_panics() {
        let key = "SENTINEL_RS_TEST_MISSING_ENV";
        std::env::remove_var(key);
        let result = std::panic::catch_unwind(|| env_required(key));
        assert!(result.is_err());
    }

    #[test]
    fn format_message_includes_fields() {
        let body = format_message("2025-01-01 00:00:00", "host", "hello");
        assert_eq!(body, "[2025-01-01 00:00:00] [host]\nhello");
    }

    #[test]
    fn telegram_payload_is_expected_shape() {
        let payload = telegram_payload("123", "body");
        assert_eq!(payload["chat_id"], "123");
        assert_eq!(payload["text"], "body");
        assert_eq!(payload["disable_web_page_preview"], true);
    }

    #[test]
    fn run_bash_captures_stdout_and_stderr() {
        let output = run_bash_with_tee("printf 'out'; printf 'err' 1>&2", false);
        assert!(output.status.success());
        assert_eq!(output.stdout, b"out");
        assert_eq!(output.stderr, b"err");
    }

    #[test]
    fn run_bash_captures_non_zero_exit() {
        let output = run_bash_with_tee("exit 7", false);
        assert_eq!(output.status.code(), Some(7));
    }
}
