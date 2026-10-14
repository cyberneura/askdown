//! The Markdown file being asked about: reading it and writing the answers back.

use std::io;
use std::path::{Path, PathBuf};

use askdown_core::{AnswerSet, Document, parse, write_back};

/// A file written next to the Markdown but not yet given its name. Dropping it
/// unwritten removes it.
#[derive(Debug)]
pub struct Staged {
    temporary: PathBuf,
    name: String,
}

impl Drop for Staged {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.temporary);
    }
}

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

    /// Writes a file next to the Markdown under a temporary name, to be moved
    /// into place by [`Session::commit_file`] (or thrown away by dropping the
    /// [`Staged`]). `name` must be a plain file name, not a path.
    ///
    /// Two steps so that the Markdown can be saved in between: the Markdown is
    /// the record, and a picture must not replace the one an earlier answer
    /// refers to unless the new answer was written.
    pub fn stage_file(&self, name: &str, bytes: &[u8]) -> io::Result<Staged> {
        if name.is_empty() || name.contains(['/', '\\']) {
            return Err(io::Error::other(format!("{name:?} is not a file name")));
        }
        let temporary = self
            .base_dir()
            .join(format!(".{name}.askdown-{}", std::process::id()));
        std::fs::write(&temporary, bytes)?;
        Ok(Staged {
            temporary,
            name: name.to_string(),
        })
    }

    /// Moves a staged file to its name. The rename replaces the directory
    /// entry, so an existing symlink or hard link of that name is swapped out
    /// rather than followed, and a crash leaves no half-written file.
    pub fn commit_file(&self, staged: Staged) -> io::Result<()> {
        let result = std::fs::rename(&staged.temporary, self.base_dir().join(&staged.name));
        // Either way the temporary is gone: moved, or removed by the drop.
        drop(staged);
        result
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
