# sentinel-rs

A secure, local task runner with out-of-band notifications, written in Rust.
It is a minimal, security-conscious command execution monitor for long-running jobs.

## Scope
- Single-user
- Local machine only
- No daemon
- No remote shell or execution

## Decisions (the "why")
### Why Telegram?
It is a simple, reliable out-of-band channel that does not require opening any
ports or maintaining a server.

### Why no polling?
There is no long-lived polling loop. Messages are sent only on start and finish,
which keeps the process simple and avoids background daemons.

### Why no remote execution?
The goal is to run trusted commands locally and receive notifications, not to
expose a remote shell or expand the attack surface.

### Why truncate logs?
Telegram has message size limits and long-running jobs can emit large output.
Truncation keeps notifications useful and bounded.

### Why byte-based truncation?
When truncation is added, it should be byte-based to enforce hard limits and to
avoid expensive transformations. It should still respect UTF-8 boundaries.

## Requirements
- Rust (stable)
- Telegram bot token and chat ID

## Setup
1) Create a Telegram bot and get a bot token.
2) Get your chat ID.
3) Export the required environment variables:

```bash
export TG_BOT_TOKEN="..."
export TG_CHAT_ID="..."
```

## Usage
```bash
cargo run -- "echo hello"
```

You can pass any shell command as the argument.

## Notes
- The command is executed via `bash -c`.
- HTTP requests use a 10s timeout.

## What I'd add next
- `/ping` command to verify connectivity
- Job registry for querying recent runs
- Failure-only notifications mode

## License
Apache-2.0

Copyright 2025 Ayush Bindlish
