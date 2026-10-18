use std::process::ExitCode;

fn main() -> ExitCode {
    match reedi::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("reedi: {error}");
            ExitCode::FAILURE
        }
    }
}
