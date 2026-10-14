//! The window that asks the questions.

use std::collections::BTreeMap;
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

#[cfg(test)]
use serde_json::json;

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

/// `drawings` holds the PNG of each `draw` question (keyed by id, as a data
/// URL) that was drawn on in this session; its path goes into the answer here.
/// Ordered by id so that a failure part-way is reproducible.
#[tauri::command]
fn submit(
    app: AppHandle,
    state: State<'_, GuiState>,
    mut answers: AnswerSet,
    drawings: BTreeMap<String, String>,
) -> Result<(), String> {
    {
        let mut session = state.session.lock().unwrap_or_else(PoisonError::into_inner);
        commit(&mut session, state.write_back, &mut answers, &drawings)?;
    }
    *state.answers.lock().unwrap_or_else(PoisonError::into_inner) = Some(answers);
    app.exit(0);
    Ok(())
}

/// Writes the answers into the Markdown and the drawings next to it, in that
/// order. A drawing replaces whatever its name held only once the answer that
/// refers to it is on disk; if the Markdown cannot be saved, no picture
/// changes, and if a picture then cannot be put in place, the Markdown and the
/// pictures already replaced are put back as they were. The pictures are
/// written even with `--no-write`: that option keeps the Markdown untouched,
/// and a picture is a new file that *is* the answer.
fn commit(
    session: &mut Session,
    write_back: bool,
    answers: &mut AnswerSet,
    drawings: &BTreeMap<String, String>,
) -> Result<(), String> {
    let mut staged = Vec::new();
    for (id, png) in drawings {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(png.strip_prefix("data:image/png;base64,").unwrap_or(png))
            .map_err(|error| format!("{id}: bad PNG data: {error}"))?;
        let name = drawing_file_name(session.path(), id);
        let Some(serde_json::Value::Object(answer)) = answers.0.get_mut(id) else {
            return Err(format!("{id}: a drawing was sent without its answer"));
        };
        answer.insert("path".to_string(), serde_json::Value::String(name.clone()));
        staged.push(
            session
                .stage_file(&name, &bytes)
                .map_err(|error| format!("could not save {name}: {error}"))?,
        );
    }
    let before = if write_back {
        Some(
            session
                .save(answers)
                .map_err(|error| format!("could not write the answers back: {error}"))?,
        )
    } else {
        None
    };
    let mut replaced = Vec::new();
    for file in staged {
        match session.commit_file(file) {
            Ok(done) => replaced.push(done),
            Err(error) => {
                // The Markdown first: if it cannot be put back (someone edited
                // it since the save), the answers in it stand, and so must the
                // pictures they refer to. Otherwise everything goes back.
                let markdown_restored = match &before {
                    Some(before) => session.restore(before).is_ok(),
                    None => true,
                };
                if markdown_restored {
                    for done in replaced.into_iter().rev() {
                        let _ = session.undo(done);
                    }
                    return Err(format!("could not save a drawing: {error}"));
                }
                for done in replaced {
                    session.finish(done);
                }
                return Err(format!(
                    "could not save a drawing: {error}. The answers were written and the file \
                     was then edited, so they were left as written; the picture of this answer \
                     is not the new one"
                ));
            }
        }
    }
    for done in replaced {
        session.finish(done);
    }
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
    if !is_web_link(&url) {
        return Err(format!("only http(s) links are opened: {url}"));
    }
    open::that_detached(&url).map_err(|error| format!("could not open {url}: {error}"))
}

/// Schemes are case-insensitive, and the window lets `HTTPS://...` through.
fn is_web_link(url: &str) -> bool {
    let scheme = url
        .split_once("://")
        .map(|(scheme, _)| scheme.to_ascii_lowercase());
    matches!(scheme.as_deref(), Some("http" | "https"))
}

/// `<markdown stem>.<question id>.png`, with the id reduced to lower-case
/// characters that are safe in a file name (it comes from the YAML, so it
/// could hold anything).
///
/// An id that had to be changed gets a hash of the original appended after a
/// `.`, so that `a.b` and `a/b` do not end up overwriting each other's picture.
/// The `.` cannot occur in an id that was left alone, so the two forms never
/// name the same file either. Lower-casing counts as a change: on the usual
/// macOS and Windows file systems `Sketch` and `sketch` are one file.
///
/// A name that would not fit a file system's limit for one name is cut down
/// to a prefix of each part plus a hash of the whole.
fn drawing_file_name(markdown: &Path, id: &str) -> String {
    let stem = markdown
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_else(|| "askdown".to_string());
    let safe: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || "-_".contains(c) {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect();
    // FNV-1a, all 64 bits: dependency-free, and wide enough that two ids
    // with the same sanitised form do not share a file in practice.
    let fnv = |text: &str| {
        text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
        })
    };
    let name = if safe == id && !safe.is_empty() {
        format!("{stem}.{safe}.png")
    } else {
        format!("{stem}.{safe}.{:016x}.png", fnv(id))
    };
    // 255 bytes is the usual limit; well under it, since the rest of the
    // path and the temporary names are not counted here.
    const LONGEST: usize = 200;
    if name.len() <= LONGEST {
        return name;
    }
    fn head(text: &str, bytes: usize) -> &str {
        let mut end = bytes.min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        &text[..end]
    }
    format!(
        "{}.{}.{:016x}.png",
        head(&stem, 64),
        head(&safe, 64),
        fnv(&format!("{stem}\0{id}"))
    )
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
            document, submit, cancel, open_link
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
    fn only_web_links_are_opened_whatever_their_case() {
        assert!(is_web_link("https://example.com/x"));
        assert!(is_web_link("HTTPS://example.com"));
        assert!(is_web_link("Http://example.com"));
        assert!(!is_web_link("file:///etc/passwd"));
        assert!(!is_web_link("javascript:alert(1)"));
        assert!(!is_web_link("httpx://example.com"));
        assert!(!is_web_link("example.com"));
    }

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
        // Ids that differ only in case are one file on a case-folding file system.
        let upper = drawing_file_name(Path::new("/tmp/questions.md"), "Sketch");
        let lower = drawing_file_name(Path::new("/tmp/questions.md"), "sketch");
        assert_eq!(lower, "questions.sketch.png");
        assert!(upper.starts_with("questions.sketch."), "{upper}");
        assert_ne!(upper.to_lowercase(), lower);
        assert_ne!(
            upper.to_lowercase(),
            drawing_file_name(Path::new("/tmp/questions.md"), "SKETCH").to_lowercase()
        );
        // A long Markdown name is cut down, deterministically and still apart.
        let long = format!("/tmp/{}.md", "m".repeat(250));
        let cut = drawing_file_name(Path::new(&long), "sketch");
        assert!(cut.len() <= 200, "{}", cut.len());
        assert!(cut.starts_with(&"m".repeat(64)), "{cut}");
        assert_eq!(cut, drawing_file_name(Path::new(&long), "sketch"));
        assert_ne!(cut, drawing_file_name(Path::new(&long), "other"));
        assert_ne!(
            cut,
            drawing_file_name(
                Path::new(&format!("/tmp/{}x.md", "m".repeat(250))),
                "sketch"
            )
        );
        // Same sanitised form, different ids: still different files.
        assert_ne!(
            drawing_file_name(Path::new("/tmp/questions.md"), "a.b$b]b}z"),
            drawing_file_name(Path::new("/tmp/questions.md"), "a!b#b?b#z")
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

    fn scratch(test: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("askdown-gui-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("q.md"),
            "```askdown\nid: sketch\ntype: draw\n```\n",
        )
        .unwrap();
        dir
    }

    fn drawing() -> BTreeMap<String, String> {
        BTreeMap::from([("sketch".to_string(), "iVBORw0KGgo=".to_string())])
    }

    fn answer() -> AnswerSet {
        AnswerSet(
            [("sketch".to_string(), json!({ "width": 1, "height": 1 }))]
                .into_iter()
                .collect(),
        )
    }

    fn no_temporaries(dir: &Path) -> bool {
        !std::fs::read_dir(dir).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with('.')
        })
    }

    #[test]
    fn a_drawing_lands_next_to_the_markdown_and_in_the_answer() {
        let dir = scratch("lands");
        let mut session = Session::load(&dir.join("q.md")).unwrap();
        let mut answers = answer();
        commit(&mut session, true, &mut answers, &drawing()).unwrap();
        assert_eq!(answers.0["sketch"]["path"], "q.sketch.png");
        assert_eq!(
            std::fs::read(dir.join("q.sketch.png")).unwrap(),
            b"\x89PNG\r\n\x1a\n"
        );
        assert!(
            std::fs::read_to_string(dir.join("q.md"))
                .unwrap()
                .contains("path: q.sketch.png")
        );
        assert!(no_temporaries(&dir));
    }

    #[cfg(unix)]
    #[test]
    fn a_drawing_replaces_a_symlink_instead_of_writing_through_it() {
        let dir = scratch("symlink");
        let target = dir.join("elsewhere.txt");
        std::fs::write(&target, "keep").unwrap();
        std::os::unix::fs::symlink(&target, dir.join("q.sketch.png")).unwrap();
        let mut session = Session::load(&dir.join("q.md")).unwrap();
        commit(&mut session, true, &mut answer(), &drawing()).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "keep");
        let saved = dir.join("q.sketch.png");
        assert!(!saved.symlink_metadata().unwrap().is_symlink());
        assert_eq!(std::fs::read(&saved).unwrap(), b"\x89PNG\r\n\x1a\n");
    }

    #[cfg(unix)]
    #[test]
    fn a_symlink_to_a_directory_is_replaced_like_any_other_link() {
        let dir = scratch("dirlink");
        std::fs::create_dir(dir.join("folder")).unwrap();
        std::os::unix::fs::symlink(dir.join("folder"), dir.join("q.sketch.png")).unwrap();
        let mut session = Session::load(&dir.join("q.md")).unwrap();
        commit(&mut session, true, &mut answer(), &drawing()).unwrap();
        assert!(
            dir.join("q.sketch.png")
                .symlink_metadata()
                .unwrap()
                .is_file()
        );
        assert!(dir.join("folder").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn a_backslash_in_the_markdown_name_is_just_a_character() {
        let dir = scratch("backslash");
        std::fs::rename(dir.join("q.md"), dir.join("re\\view.md")).unwrap();
        let mut session = Session::load(&dir.join("re\\view.md")).unwrap();
        let mut answers = answer();
        commit(&mut session, true, &mut answers, &drawing()).unwrap();
        assert_eq!(answers.0["sketch"]["path"], "re\\view.sketch.png");
        assert!(dir.join("re\\view.sketch.png").is_file());
    }

    #[test]
    fn a_name_near_the_file_system_limit_still_goes_through_staging() {
        let dir = scratch("long");
        let id = "a".repeat(240);
        let mut answers = AnswerSet([(id.clone(), json!({}))].into_iter().collect());
        let drawings = BTreeMap::from([(id.clone(), "iVBORw0KGgo=".to_string())]);
        let mut session = Session::load(&dir.join("q.md")).unwrap();
        commit(&mut session, false, &mut answers, &drawings).unwrap();
        let path = answers.0[&id]["path"].as_str().unwrap();
        assert!(path.len() <= 200, "{path}");
        assert!(dir.join(path).is_file());
        assert!(no_temporaries(&dir));
    }

    /// Needs a way to make one rename fail after another succeeded: on macOS
    /// a file with the immutable flag cannot be renamed (or renamed over).
    #[cfg(target_os = "macos")]
    #[test]
    fn a_later_failure_puts_back_the_pictures_replaced_before_it() {
        let dir = scratch("rollback");
        std::fs::write(
            dir.join("q.md"),
            "```askdown\nid: first\ntype: draw\n```\n\n```askdown\nid: second\ntype: draw\n```\n",
        )
        .unwrap();
        std::fs::write(dir.join("q.first.png"), "old picture").unwrap();
        std::fs::write(dir.join("q.second.png"), "locked").unwrap();
        let chflags = |flag: &str| {
            assert!(
                std::process::Command::new("chflags")
                    .arg(flag)
                    .arg(dir.join("q.second.png"))
                    .status()
                    .unwrap()
                    .success()
            );
        };
        chflags("uchg");
        let mut session = Session::load(&dir.join("q.md")).unwrap();
        let mut answers = AnswerSet(
            [
                ("first".to_string(), json!({})),
                ("second".to_string(), json!({})),
            ]
            .into_iter()
            .collect(),
        );
        let drawings = BTreeMap::from([
            ("first".to_string(), "iVBORw0KGgo=".to_string()),
            ("second".to_string(), "iVBORw0KGgo=".to_string()),
        ]);
        let before = std::fs::read_to_string(dir.join("q.md")).unwrap();
        let result = commit(&mut session, true, &mut answers, &drawings);
        chflags("nouchg");
        assert!(result.is_err(), "{result:?}");
        assert_eq!(
            std::fs::read_to_string(dir.join("q.first.png")).unwrap(),
            "old picture"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("q.second.png")).unwrap(),
            "locked"
        );
        assert_eq!(std::fs::read_to_string(dir.join("q.md")).unwrap(), before);
        assert!(no_temporaries(&dir));
    }

    #[test]
    fn a_rollback_leaves_an_edit_made_after_the_save_alone() {
        let dir = scratch("edited");
        let mut session = Session::load(&dir.join("q.md")).unwrap();
        let before = session.save(&answer()).unwrap();
        std::fs::write(dir.join("q.md"), "someone else's text\n").unwrap();
        assert!(session.restore(&before).is_err());
        assert_eq!(
            std::fs::read_to_string(dir.join("q.md")).unwrap(),
            "someone else's text\n"
        );
    }

    #[test]
    fn a_leftover_temporary_of_the_same_name_is_not_touched() {
        let dir = scratch("leftover");
        // Whatever name the generator would hand out first, it is taken.
        let pid = std::process::id();
        for n in 0..64 {
            std::fs::write(dir.join(format!(".askdown-{pid}-{n}.tmp")), "leftover").unwrap();
        }
        let mut session = Session::load(&dir.join("q.md")).unwrap();
        commit(&mut session, false, &mut answer(), &drawing()).unwrap();
        assert!(dir.join("q.sketch.png").is_file());
        for n in 0..64 {
            assert_eq!(
                std::fs::read_to_string(dir.join(format!(".askdown-{pid}-{n}.tmp"))).unwrap(),
                "leftover",
                "{n}"
            );
        }
    }

    #[test]
    fn a_directory_in_the_way_is_refused_before_the_markdown_is_written() {
        let dir = scratch("directory");
        std::fs::create_dir(dir.join("q.sketch.png")).unwrap();
        let mut session = Session::load(&dir.join("q.md")).unwrap();
        let result = commit(&mut session, true, &mut answer(), &drawing());
        assert!(result.is_err(), "{result:?}");
        assert!(dir.join("q.sketch.png").is_dir());
        assert!(
            !std::fs::read_to_string(dir.join("q.md"))
                .unwrap()
                .contains("askdown:answer")
        );
        assert!(no_temporaries(&dir));
    }

    #[test]
    fn a_drawing_is_not_replaced_when_the_markdown_cannot_be_saved() {
        let dir = scratch("unsaved");
        std::fs::write(dir.join("q.sketch.png"), "old picture").unwrap();
        let mut session = Session::load(&dir.join("q.md")).unwrap();
        // The Markdown goes away while the window is open.
        std::fs::remove_file(dir.join("q.md")).unwrap();
        std::fs::create_dir(dir.join("q.md")).unwrap();
        let result = commit(&mut session, true, &mut answer(), &drawing());
        assert!(result.is_err(), "{result:?}");
        assert_eq!(
            std::fs::read_to_string(dir.join("q.sketch.png")).unwrap(),
            "old picture"
        );
        assert!(no_temporaries(&dir));
    }
}
