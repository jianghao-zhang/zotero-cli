use std::process::ExitCode;

use zcli::{run, Cli};

fn main() -> ExitCode {
    let cli = <Cli as clap::Parser>::parse();
    match run(cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            let payload = serde_json::json!({
                "ok": false,
                "error": {
                    "code": "command_failed",
                    "message": error.to_string(),
                }
            });
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| error.to_string())
            );
            ExitCode::from(1)
        }
    }
}
