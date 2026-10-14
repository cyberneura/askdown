//! The window that asks the questions.

use std::io;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use askdown_core::{AnswerSet, Question, Segment, answer, markdown};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::session::Session;

/// How the window was closed.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Answered(AnswerSet),
    /// Closed, or cancelled, without submitting.
    Cancelled,
    TimedOut,
}

pub const EXIT_CANCELLED: i32 = 2;
pub const EXIT_TIMED_OUT: i32 = 3;

pub struct Options {
    /// Write the answers into the Markdown file as well as reporting them.
    pub write_back: bool,
    pub timeout: Option<Duration>,
    pub title: Option<String>,
}

struct GuiState {
    session: Mutex<Session>,
    write_back: bool,
    /// Shared with `run`, which reads it once the event loop has returned.
    answers: Arc<Mutex<Option<AnswerSet>>>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
enum SegmentView {
    Prose {
        html: String,
    },
    Question {
        number: usize,
        id: String,
        question: Option<Question>,
        /// The question text, rendered.
        html: Option<String>,
        /// Why the block could not be read, when it could not.
        error: Option<String>,
        /// The answer already in the file, from an earlier run.
        previous: Option<serde_json::Value>,
    },
}

#[derive(Serialize)]
struct DocumentView {
    path: String,
    segments: Vec<SegmentView>,
}

fn document_view(session: &Session) -> DocumentView {
    let document = session.document();
    let segments = document
        .segments
        .iter()
        .map(|segment| match segment {
            Segment::Prose(text) => SegmentView::Prose {
                html: markdown::to_html(text),
            },
            Segment::Question(index) => {
                let block = &document.blocks[*index];
                let (question, error) = match &block.question {
                    Ok(question) => (Some(question.clone()), None),
                    Err(error) => (None, Some(error.clone())),
                };
                SegmentView::Question {
                    number: block.number,
                    id: block.id(),
                    html: question
                        .as_ref()
                        .and_then(|question| question.question.as_deref())
                        .map(markdown::to_html),
                    question,
                    error,
                    previous: block
                        .answer_range
                        .clone()
                        .and_then(|range| answer::read_answer(&session.source()[range])),
                }
            }
        })
        .collect();
    DocumentView {
        path: session.path().display().to_string(),
        segments,
    }
}

#[tauri::command]
fn document(state: State<'_, GuiState>) -> DocumentView {
    document_view(&state.session.lock().unwrap_or_else(PoisonError::into_inner))
}

#[tauri::command]
fn submit(app: AppHandle, state: State<'_, GuiState>, answers: AnswerSet) -> Result<(), String> {
    if state.write_back {
        state
            .session
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .save(&answers)
            .map_err(|error| format!("could not write the answers back: {error}"))?;
    }
    *state.answers.lock().unwrap_or_else(PoisonError::into_inner) = Some(answers);
    app.exit(0);
    Ok(())
}

#[tauri::command]
fn cancel(app: AppHandle) {
    app.exit(EXIT_CANCELLED);
}

/// Opens the window and blocks until it is answered, cancelled or timed out.
pub fn run(session: Session, options: Options) -> io::Result<Outcome> {
    let title = options.title.unwrap_or_else(|| {
        let name = session
            .path()
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        format!("{name} - askdown")
    });

    let answers = Arc::new(Mutex::new(None));
    let app = tauri::Builder::default()
        .manage(GuiState {
            session: Mutex::new(session),
            write_back: options.write_back,
            answers: Arc::clone(&answers),
        })
        .setup(move |app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title(&title);
                // Started by an agent from a terminal, the window would otherwise
                // open behind it and the user would not notice they are being asked.
                let _ = window.set_focus();
            }
            if let Some(timeout) = options.timeout {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(timeout);
                    handle.exit(EXIT_TIMED_OUT);
                });
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![document, submit, cancel])
        .build(tauri::generate_context!())
        .map_err(io::Error::other)?;

    let code = app.run_return(|_, _| {});
    Ok(match code {
        0 => match answers.lock().unwrap_or_else(PoisonError::into_inner).take() {
            Some(answers) => Outcome::Answered(answers),
            None => Outcome::Cancelled,
        },
        EXIT_TIMED_OUT => Outcome::TimedOut,
        _ => Outcome::Cancelled,
    })
}
