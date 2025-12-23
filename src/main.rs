use chrono::Local;
use hostname::get;
use log::info;
use reqwest::blocking::Client;
use serde_json::json;
use std::env;
use std::io::{Read, Write};
use std::process::{Command, Output, Stdio};
use std::sync::mpsc;
use std::thread;

fn env_required(key: &str) -> Result<String, std::env::VarError> {
    std::env::var(key)
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

fn tg_send(text: &str) -> Result<(), Box<dyn std::error::Error>> {
    let bot_token = env_required("TG_BOT_TOKEN")?;
    let chat_id = env_required("TG_CHAT_ID")?;
    let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
    let host = get().unwrap_or_default().to_string_lossy().to_string();
    let ts = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let body = format_message(&ts, &host, text);
    let client = Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    client
        .post(&url)
        .json(&telegram_payload(&chat_id, &body))
        .send()?;
    Ok(())
}

fn start_notifier() -> (mpsc::Sender<String>, thread::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<String>();
    let handle = thread::spawn(move || {
        for msg in rx {
            if let Err(e) = tg_send(&msg) {
                eprintln!("Failed to send telegram message: {e}");
            }
        }
    });
    (tx, handle)
}

fn read_stream<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
    tee: bool,
) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let read = reader.read(&mut chunk)?;
        if read == 0 {
            break;
        }
        if tee {
            writer.write_all(&chunk[..read])?;
            writer.flush().ok();
        }
        buf.extend_from_slice(&chunk[..read]);
    }
    Ok(buf)
}

fn run_bash_with_tee(command: &str, tee: bool) -> std::io::Result<Output> {
    let mut child = Command::new("bash")
        .arg("-c")
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| std::io::Error::other("Failed to capture stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| std::io::Error::other("Failed to capture stderr"))?;

    let stdout_handle = std::thread::spawn(move || read_stream(stdout, std::io::stdout(), tee));
    let stderr_handle = std::thread::spawn(move || read_stream(stderr, std::io::stderr(), tee));

    let status = child.wait()?;
    let out_buf = stdout_handle
        .join()
        .map_err(|_| std::io::Error::other("Failed to capture stdout"))?;
    let err_buf = stderr_handle
        .join()
        .map_err(|_| std::io::Error::other("Failed to capture stderr"))?;

    Ok(Output {
        status,
        stdout: out_buf?,
        stderr: err_buf?,
    })
}

fn run_bash(command: &str) -> std::io::Result<Output> {
    run_bash_with_tee(command, true).map_err(|e| {
        std::io::Error::new(
            e.kind(),
            format!("Failed to run bash command '{command}': {e}"),
        )
    })
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

fn print_help() {
    eprintln!(
        "Usage: sentinel-rs [--help] [--version] [-- <command>...]\n\
Runs a command via bash -c and sends Telegram notifications.\n\n\
Examples:\n\
  sentinel-rs -- \"echo hello\"\n\
  sentinel-rs -- ls -la\n\
  sentinel-rs -- --help   # runs a command named \"--help\""
    );
}

fn main() {
    env_logger::init();
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() {
        print_help();
        std::process::exit(2);
    }

    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        print_help();
        return;
    }

    if args.len() == 1 && (args[0] == "--version" || args[0] == "-V") {
        eprintln!("sentinel-rs {}", env!("CARGO_PKG_VERSION"));
        return;
    }

    let command_args = if args[0] == "--" {
        if args.len() == 1 {
            eprintln!("Missing command after --.");
            print_help();
            std::process::exit(2);
        }
        &args[1..]
    } else {
        &args[..]
    };
    let command = command_args.join(" ");

    let (notifier, handle) = start_notifier();
    notifier.send(format!("Started\n{command}")).ok();

    let output = match run_bash(&command) {
        Ok(output) => output,
        Err(e) => {
            notifier
                .send(format!("Failed to execute command: {e}"))
                .ok();
            info!("Failed to execute command: {e}");
            drop(notifier);
            handle.join().ok();
            return;
        }
    };

    match output.status.code() {
        Some(0) => {
            notifier
                .send(format!(
                    "Finished successfully with exit code 0.\nStdout:\n{}\nStderr:\n{}",
                    tail_bytes(&output.stdout, 1500),
                    tail_bytes(&output.stderr, 1500)
                ))
                .ok();
            info!("Command finished successfully with exit code 0");
        }
        Some(code) => {
            notifier
                .send(format!(
                    "Failed with exit code: {}.\nStdout:\n{}\nStderr:\n{}",
                    code,
                    tail_bytes(&output.stdout, 1500),
                    tail_bytes(&output.stderr, 1500)
                ))
                .ok();
            info!(
                "Failed with exit code: {}. Stdout: {} Stderr: {}",
                code,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
        None => {
            notifier
                .send(format!(
                    "Process terminated by signal.\nStdout:\n{}\nStderr:\n{}",
                    tail_bytes(&output.stdout, 1500),
                    tail_bytes(&output.stderr, 1500)
                ))
                .ok();
            info!("Process terminated by signal.");
        }
    }
    drop(notifier);
    handle.join().ok();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_required_present_returns_value() {
        let key = "SENTINEL_RS_TEST_ENV";
        let value = "test_value".to_string();
        let prior = std::env::var(key).ok();
        unsafe {
            std::env::set_var(key, &value);
        }
        let result = env_required(key).unwrap();
        unsafe {
            if let Some(prior) = prior {
                std::env::set_var(key, prior);
            } else {
                std::env::remove_var(key);
            }
        }
        assert_eq!(result, value);
    }

    #[test]
    fn env_required_missing_returns_err() {
        let key = "SENTINEL_RS_TEST_MISSING_ENV";
        unsafe {
            std::env::remove_var(key);
        }
        let result = env_required(key);
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
    fn tail_bytes_truncates_correctly() {
        let data = b"abcdefghijklmnopqrstuvwxyz";
        let result = tail_bytes(data, 10);
        assert_eq!(result, "… (truncated, showing last 10 bytes)\nqrstuvwxyz");
    }

    #[test]
    fn tail_bytes_no_truncation() {
        let data = b"hello";
        let result = tail_bytes(data, 10);
        assert_eq!(result, "hello");
    }

    #[test]
    fn tail_bytes_exact_boundary() {
        let data = b"exact10!!";
        let result = tail_bytes(data, 9);
        assert_eq!(result, "exact10!!");
    }

    #[test]
    fn tail_bytes_handles_non_utf8() {
        let data = [0x66, 0xff, 0x6f];
        let result = tail_bytes(&data, 10);
        assert_eq!(result, String::from_utf8_lossy(&data));
    }

    #[test]
    fn run_bash_captures_stdout_and_stderr() {
        let output = run_bash_with_tee("printf 'out'; printf 'err' 1>&2", false).unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"out");
        assert_eq!(output.stderr, b"err");
    }

    #[test]
    fn run_bash_captures_non_zero_exit() {
        let output = run_bash_with_tee("exit 7", false).unwrap();
        assert_eq!(output.status.code(), Some(7));
    }

    #[test]
    fn read_stream_no_tee_keeps_writer_empty() {
        use std::io::Cursor;
        let input_data = Cursor::new(b"hello world");
        let mut output = Vec::new();
        let buf = read_stream(input_data, &mut output, false).expect("Failed to read stream");
        assert_eq!(buf, b"hello world");
        assert!(output.is_empty());
    }

    #[test]
    fn read_stream_copies_when_tee_true() {
        use std::io::Cursor;
        let input_data = Cursor::new(b"hello world");
        let mut output = Vec::new();
        let buf = read_stream(input_data, &mut output, true).expect("Failed to read stream");
        assert_eq!(buf, b"hello world");
        assert_eq!(output, b"hello world");
    }
}
