//! The Markdown file being asked about: reading it and writing the answers back.

use std::io;
use std::path::{Path, PathBuf};

use askdown_core::{AnswerSet, Document, parse, write_back};

pub struct Session {
    path: PathBuf,
    document: Document,
    source: String,
}

impl Session {
    pub fn load(path: &Path) -> io::Result<Session> {
        let path = path.canonicalize()?;
        let source = std::fs::read_to_string(&path)?;
        let document = parse(&source);
        Ok(Session {
            path,
            document,
            source,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn document(&self) -> &Document {
        &self.document
    }

    pub fn source(&self) -> &str {
        &self.source
    }

    /// The directory relative media paths in the file are resolved against.
    pub fn base_dir(&self) -> &Path {
        self.path.parent().unwrap_or(Path::new("/"))
    }

    /// Writes a file next to the Markdown (the picture of a `draw` question).
    ///
    /// Written to a temporary name and renamed into place. The rename replaces
    /// the directory entry, so an existing symlink or hard link of that name is
    /// swapped out rather than followed, and a crash leaves no half-written file.
    /// `name` must be a plain file name, not a path.
    pub fn write_file(&self, name: &str, bytes: &[u8]) -> io::Result<()> {
        if name.is_empty() || name.contains(['/', '\\']) {
            return Err(io::Error::other(format!("{name:?} is not a file name")));
        }
        let path = self.base_dir().join(name);
        let temporary = self
            .base_dir()
            .join(format!(".{name}.askdown-{}", std::process::id()));
        std::fs::write(&temporary, bytes)?;
        std::fs::rename(&temporary, &path).inspect_err(|_| {
            let _ = std::fs::remove_file(&temporary);
        })
    }

    /// Writes the answers into the file.
    ///
    /// The file is read again first: the agent or the user may have edited it
    /// while the window was open, and writing back over the copy loaded at start
    /// would silently undo that. Answers are matched to questions by id, so they
    /// land in the right place even if blocks moved.
    pub fn save(&mut self, answers: &AnswerSet) -> io::Result<()> {
        let current = std::fs::read_to_string(&self.path)?;
        let document = parse(&current);
        let output = write_back(&current, &document, answers);
        std::fs::write(&self.path, &output)?;
        self.document = parse(&output);
        self.source = output;
        Ok(())
    }
}
