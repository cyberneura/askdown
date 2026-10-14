//! Entry point of askdown.

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use askdown::gui::{self, EXIT_CANCELLED, EXIT_TIMED_OUT, Outcome};
use askdown::session::Session;
use clap::Parser;

/// Opens a window that asks the questions written in a Markdown file, and
/// blocks until they are answered.
///
/// Questions are fenced code blocks with the info string `askdown`, holding
/// YAML. The answers are printed to stdout as JSON keyed by question id, and
/// written back into the file right after each question.
///
/// Exit codes: 0 answered, 1 error, 2 cancelled (window closed without
/// submitting), 3 timed out.
#[derive(Parser, Debug)]
#[command(name = "askdown", version, about, long_about)]
struct Args {
    /// The Markdown file holding the questions.
    file: PathBuf,

    /// Print the questions found in the file and exit, without opening a window.
    #[arg(short, long)]
    list: bool,

    /// Report the answers on stdout only; leave the file untouched.
    #[arg(long)]
    no_write: bool,

    /// Give up after this many seconds and exit with code 3.
    #[arg(long, value_name = "SECONDS")]
    timeout: Option<u64>,

    /// Window title. Defaults to the file name.
    #[arg(long)]
    title: Option<String>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    match dispatch(args) {
        Ok(code) => code,
        Err(error) => {
            eprintln!("askdown: {error}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(args: Args) -> std::io::Result<ExitCode> {
    let session = Session::load(&args.file)?;

    if args.list {
        print_list(&session);
        return Ok(ExitCode::SUCCESS);
    }
    if session.document().blocks.is_empty() {
        return Err(std::io::Error::other(format!(
            "no ```askdown question blocks in {}",
            session.path().display()
        )));
    }

    let outcome = gui::run(
        session,
        gui::Options {
            write_back: !args.no_write,
            timeout: args.timeout.map(Duration::from_secs),
            title: args.title,
        },
    )?;
    Ok(match outcome {
        Outcome::Answered(answers) => {
            let json = serde_json::to_string_pretty(&answers).map_err(std::io::Error::other)?;
            println!("{json}");
            ExitCode::SUCCESS
        }
        Outcome::Cancelled => {
            eprintln!("askdown: cancelled");
            ExitCode::from(EXIT_CANCELLED as u8)
        }
        Outcome::TimedOut => {
            eprintln!("askdown: timed out");
            ExitCode::from(EXIT_TIMED_OUT as u8)
        }
    })
}

fn print_list(session: &Session) {
    for block in &session.document().blocks {
        let summary = match &block.question {
            Ok(question) => {
                let text = question
                    .question
                    .as_deref()
                    .and_then(|text| text.lines().next())
                    .map(|line| format!("  {line}"))
                    .unwrap_or_default();
                format!("{:?}{text}", question.kind)
            }
            Err(error) => format!("invalid: {}", error.lines().next().unwrap_or("")),
        };
        let answered = if block.answer_range.is_some() { " (answered)" } else { "" };
        println!("[{}] {}: {summary}{answered}", block.number, block.id());
    }
}
