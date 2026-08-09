use std::io::IsTerminal;
use std::process::ExitCode;

use crate::{exec, graph, output};

#[derive(Debug, Eq, PartialEq)]
enum Mode {
    Inspect(Vec<String>),
    All,
}

struct Args {
    mode: Mode,
    explain: bool,
    json: bool,
    show_missing: bool,
}

pub fn run(program: &str, arguments: impl Iterator<Item = String>) -> ExitCode {
    let args = match parse_args(program, arguments) {
        Ok(Some(args)) => args,
        Ok(None) => return ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}\n\n{}", help(program));
            return ExitCode::from(2);
        }
    };

    let runner = exec::CommandRunner::new();
    let graphs = match &args.mode {
        Mode::Inspect(commands) => commands
            .iter()
            .map(|command| graph::inspect(command, &runner))
            .collect(),
        Mode::All => graph::all(args.show_missing, &runner),
    };

    let stdout = std::io::stdout();
    let color = stdout.is_terminal();
    let result = if args.json {
        output::print_json(&graphs, stdout.lock())
    } else {
        match args.mode {
            Mode::Inspect(_) => output::print_inspect(&graphs, args.explain, color, stdout.lock()),
            Mode::All => output::print_list(&graphs, args.explain, color, stdout.lock()),
        }
    };
    if let Err(error) = result {
        eprintln!("error: could not write output: {error}");
        return ExitCode::FAILURE;
    }

    // A timed-out or unreadable manager query never fails the run; it is
    // still surfaced here so it does not degrade silently.
    for diagnostic in runner.diagnostics() {
        eprintln!("note: {diagnostic}");
    }

    if matches!(args.mode, Mode::Inspect(_))
        && graphs.iter().any(|graph| graph.resolutions.is_empty())
    {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn help(program: &str) -> String {
    format!(
        "{program} — explain who owns a command on PATH

USAGE:
    {program} [OPTIONS] <COMMAND ...>
    {program} --all [OPTIONS]

ARGS:
    <COMMAND ...>    Executable names or paths to inspect

OPTIONS:
    --all            Summarize common development runtimes
    --explain        Show detailed evidence and every ownership layer
    --json           Emit the versioned OwnershipGraph JSON document
    --show-missing   Include missing runtimes with --all
    -h, --help       Print help
    -V, --version    Print version

Individual lookup is the primary workflow. No update or removal command is
executed; action guides are printed for the user to review."
    )
}

fn parse_args(
    program: &str,
    arguments: impl Iterator<Item = String>,
) -> Result<Option<Args>, String> {
    let mut explain = false;
    let mut json = false;
    let mut show_missing = false;
    let mut all = false;
    let mut positional = Vec::new();
    let mut positional_only = false;
    for argument in arguments {
        if positional_only {
            positional.push(argument);
            continue;
        }
        match argument.as_str() {
            "--" => positional_only = true,
            "--explain" => explain = true,
            "--json" => json = true,
            "--all" => all = true,
            "--show-missing" => show_missing = true,
            "-h" | "--help" => {
                println!("{}", help(program));
                return Ok(None);
            }
            "-V" | "--version" => {
                println!("{program} {}", env!("CARGO_PKG_VERSION"));
                return Ok(None);
            }
            value if value.starts_with('-') => return Err(format!("unknown option: {value}")),
            value => positional.push(value.into()),
        }
    }

    let mode = match (all, positional.as_slice()) {
        (true, []) => Mode::All,
        (true, _) => return Err("--all does not accept command names".into()),
        (false, []) => return Err("missing COMMAND; inspect one command or use --all".into()),
        (false, commands) => Mode::Inspect(commands.to_vec()),
    };
    if show_missing && mode != Mode::All {
        return Err("--show-missing is only valid with --all".into());
    }

    Ok(Some(Args {
        mode,
        explain,
        json,
        show_missing,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_uses_invoked_binary_name() {
        assert!(help("whowns").contains("whowns [OPTIONS] <COMMAND ...>"));
        assert!(help("whowns").contains("whowns --all [OPTIONS]"));
    }

    #[test]
    fn parses_individual_lookup_as_primary_mode() {
        let args = parse_args(
            "whowns",
            ["--explain", "node", "python3"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap()
        .unwrap();
        assert!(args.explain);
        assert_eq!(
            args.mode,
            Mode::Inspect(vec!["node".into(), "python3".into()])
        );
    }

    #[test]
    fn parses_all_option_as_all_mode() {
        let args = parse_args(
            "whowns",
            ["--all", "--json", "--show-missing"]
                .into_iter()
                .map(str::to_owned),
        )
        .unwrap()
        .unwrap();
        assert!(args.json);
        assert!(args.show_missing);
        assert_eq!(args.mode, Mode::All);
    }

    #[test]
    fn rejects_all_with_command_names() {
        let result = parse_args("whowns", ["node", "--all"].into_iter().map(str::to_owned));
        assert_eq!(
            result.err().as_deref(),
            Some("--all does not accept command names")
        );
    }

    #[test]
    fn requires_a_primary_command() {
        let result = parse_args("whowns", std::iter::empty());
        assert!(result.is_err());
    }

    #[test]
    fn rejects_unknown_options() {
        let result = parse_args("whowns", ["--wat"].into_iter().map(str::to_owned));
        assert!(result.is_err());
    }
}
