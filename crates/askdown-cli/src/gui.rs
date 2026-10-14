//! The window that asks the questions.

use std::io;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use askdown_core::{AnswerSet, Question, Segment, answer, markdown};
use base64::Engine;
use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::media;
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
        question: Option<Box<Question>>,
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
    /// Prefix that turns a path relative to the Markdown file into a URL the
    /// window can load (see [`media`]).
    media_base: String,
    segments: Vec<SegmentView>,
}

fn document_view(session: &Session) -> DocumentView {
    let document = session.document();
    let media_base = media::base_url();
    let segments = document
        .segments
        .iter()
        .map(|segment| match segment {
            Segment::Prose(text) => SegmentView::Prose {
                html: markdown::to_html(text, media_base),
            },
            Segment::Question(index) => {
                let block = &document.blocks[*index];
                let (question, error) = match &block.question {
                    Ok(question) => (Some(Box::new(question.clone())), None),
                    Err(error) => (None, Some(error.clone())),
                };
                SegmentView::Question {
                    number: block.number,
                    id: block.id(),
                    html: question
                        .as_ref()
                        .and_then(|question| question.question.as_deref())
                        .map(|text| markdown::to_html(text, media_base)),
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
        media_base: media_base.to_string(),
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

/// Opens a link from the Markdown in the system browser.
///
/// Only web links: the Markdown is written by an agent, and `file:` or custom
/// schemes could start arbitrary programs.
#[tauri::command]
fn open_link(url: String) -> Result<(), String> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err(format!("only http(s) links are opened: {url}"));
    }
    open::that_detached(&url).map_err(|error| format!("could not open {url}: {error}"))
}

/// Saves the picture of a `draw` question next to the Markdown file and returns
/// its path relative to the file, which is what goes into the answer.
///
/// Written even with `--no-write`: that option keeps the Markdown untouched,
/// and the picture is a new file that *is* the answer.
#[tauri::command]
fn save_drawing(state: State<'_, GuiState>, id: String, png: String) -> Result<String, String> {
    save_to(&state, &id, &png)
}

fn save_to(state: &GuiState, id: &str, png: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(png.strip_prefix("data:image/png;base64,").unwrap_or(png))
        .map_err(|error| format!("bad PNG data: {error}"))?;
    let session = state.session.lock().unwrap_or_else(PoisonError::into_inner);
    let name = drawing_file_name(session.path(), id);
    session
        .write_file(&name, &bytes)
        .map_err(|error| format!("could not save {name}: {error}"))?;
    Ok(name)
}

/// `<markdown stem>.<question id>.png`, with the id reduced to characters that
/// are safe in a file name (it comes from the YAML, so it could hold anything).
///
/// An id that had to be changed gets a hash of the original appended after a
/// `.`, so that `a.b` and `a/b` do not end up overwriting each other's picture.
/// The `.` cannot occur in an id that was left alone, so the two forms never
/// name the same file either.
fn drawing_file_name(markdown: &Path, id: &str) -> String {
    let stem = markdown
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "askdown".to_string());
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_".contains(c) {
                c
            } else {
                '_'
            }
        })
        .collect();
    if safe == id && !safe.is_empty() {
        format!("{stem}.{safe}.png")
    } else {
        // FNV-1a: short, dependency-free, and only has to tell ids apart.
        let hash = id.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        });
        format!("{stem}.{safe}.{:06x}.png", hash & 0xff_ffff)
    }
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
        .register_uri_scheme_protocol(media::SCHEME, |context, request| {
            let state = context.app_handle().state::<GuiState>();
            let dir = state
                .session
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .base_dir()
                .to_path_buf();
            media::respond(&dir, &request)
        })
        .invoke_handler(tauri::generate_handler![
            document,
            submit,
            cancel,
            open_link,
            save_drawing
        ])
        .build(tauri::generate_context!())
        .map_err(io::Error::other)?;

    let code = app.run_return(|_, _| {});
    Ok(match code {
        0 => match answers
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            Some(answers) => Outcome::Answered(answers),
            None => Outcome::Cancelled,
        },
        EXIT_TIMED_OUT => Outcome::TimedOut,
        _ => Outcome::Cancelled,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drawing_file_names_come_from_the_markdown_and_the_question() {
        assert_eq!(
            drawing_file_name(Path::new("/tmp/questions.md"), "sketch"),
            "questions.sketch.png"
        );
        let dotted = drawing_file_name(Path::new("/tmp/questions.md"), "a.b");
        let slashed = drawing_file_name(Path::new("/tmp/questions.md"), "a/b");
        assert!(dotted.starts_with("questions.a_b."), "{dotted}");
        assert!(dotted.ends_with(".png"));
        assert_ne!(dotted, slashed);
        assert_eq!(
            dotted,
            drawing_file_name(Path::new("/tmp/questions.md"), "a.b")
        );
        // An untouched id can never spell the hashed form of another one.
        let hashed = dotted
            .trim_start_matches("questions.")
            .trim_end_matches(".png");
        assert_ne!(
            drawing_file_name(Path::new("/tmp/questions.md"), hashed),
            dotted
        );
    }

    #[test]
    fn a_drawing_replaces_a_symlink_instead_of_writing_through_it() {
        let dir = std::env::temp_dir().join(format!("askdown-draw-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let markdown = dir.join("q.md");
        std::fs::write(&markdown, "```askdown\ntype: draw\n```\n").unwrap();
        let target = dir.join("elsewhere.txt");
        std::fs::write(&target, "keep").unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&target, dir.join("q.sketch.png")).unwrap();
        #[cfg(not(unix))]
        return;

        let state = GuiState {
            session: Mutex::new(Session::load(&markdown).unwrap()),
            write_back: true,
            answers: Arc::new(Mutex::new(None)),
        };
        let result = save_to(&state, "sketch", "iVBORw0KGgo=");
        assert_eq!(result.as_deref(), Ok("q.sketch.png"));
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");
        let saved = dir.join("q.sketch.png");
        assert!(!saved.symlink_metadata().unwrap().is_symlink());
        assert_eq!(std::fs::read(&saved).unwrap(), b"\x89PNG\r\n\x1a\n");
        assert!(!std::fs::read_dir(&dir).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with('.')
        }));
    }
}
