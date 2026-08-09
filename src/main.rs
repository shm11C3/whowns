use std::env;
use std::process::ExitCode;

fn main() -> ExitCode {
    whowns::run("whowns", env::args().skip(1))
}
