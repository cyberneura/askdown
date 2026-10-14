//! The Markdown file being asked about: reading it and writing the answers back.

use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

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

/// A staged file moved to its name, with whatever that name held before set
/// aside, so that the move can still be undone ([`Session::undo`]) until it
/// is made final ([`Session::finish`]).
#[derive(Debug)]
pub struct Replaced {
    path: PathBuf,
    previous: Option<PathBuf>,
}

/// Candidate temporary names, short and unrelated to any drawing's name so
/// that a name near the file system's limit is not pushed over it. A name may
/// still be taken (a crash left it, and the pid came round again), so callers
/// take the first that is not.
fn temporary_name() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        ".askdown-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
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
        if name.is_empty() || name.chars().any(std::path::is_separator) {
            return Err(io::Error::other(format!("{name:?} is not a file name")));
        }
        // The one thing a rename cannot replace. Found out now, before the
        // answer that refers to the file is written. A symlink to a directory
        // is fine: the rename swaps out the link itself.
        let in_the_way = self.base_dir().join(name).symlink_metadata();
        if in_the_way.is_ok_and(|metadata| metadata.is_dir()) {
            return Err(io::Error::other(format!("{name} is a directory")));
        }
        // `create_new`: a leftover of the same name is not truncated (nor
        // followed, if it is a link); the next name is tried instead.
        let (temporary, mut file) = loop {
            let candidate = self.base_dir().join(temporary_name());
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&candidate)
            {
                Ok(file) => break (candidate, file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        };
        let staged = Staged {
            temporary,
            name: name.to_string(),
        };
        std::io::Write::write_all(&mut file, bytes)?;
        file.sync_all()?;
        Ok(staged)
    }

    /// Moves a staged file to its name. The rename replaces the directory
    /// entry, so an existing symlink or hard link of that name is swapped out
    /// rather than followed, and a crash leaves no half-written file.
    ///
    /// What the name held before is set aside under a temporary name first,
    /// so that the move can be undone if a later step fails.
    pub fn commit_file(&self, staged: Staged) -> io::Result<Replaced> {
        let path = self.base_dir().join(&staged.name);
        let previous = if path.symlink_metadata().is_ok() {
            // An unused name, so that nothing (such as another set-aside file)
            // is renamed over.
            let aside = loop {
                let candidate = self.base_dir().join(temporary_name());
                if candidate.symlink_metadata().is_err() {
                    break candidate;
                }
            };
            std::fs::rename(&path, &aside)?;
            Some(aside)
        } else {
            None
        };
        if let Err(error) = std::fs::rename(&staged.temporary, &path) {
            if let Some(aside) = &previous {
                let _ = std::fs::rename(aside, &path);
            }
            return Err(error);
        }
        // The temporary is gone (moved), so there is nothing for the drop to remove.
        drop(staged);
        Ok(Replaced { path, previous })
    }

    /// Puts back what a name held before [`Session::commit_file`].
    pub fn undo(&self, replaced: Replaced) -> io::Result<()> {
        match replaced.previous {
            Some(aside) => std::fs::rename(aside, &replaced.path),
            None => std::fs::remove_file(&replaced.path),
        }
    }

    /// Lets go of what a name held before [`Session::commit_file`].
    pub fn finish(&self, replaced: Replaced) {
        if let Some(aside) = replaced.previous {
            let _ = std::fs::remove_file(aside);
        }
    }

    /// Puts the file back to `source`, as read before a save that then had to
    /// be undone -- unless the file has changed since that save, in which case
    /// someone else's edit is in it and is left alone.
    pub fn restore(&mut self, source: &str) -> io::Result<()> {
        if std::fs::read_to_string(&self.path)? != self.source {
            return Err(io::Error::other("the file was edited after the save"));
        }
        std::fs::write(&self.path, source)?;
        self.document = parse(source);
        self.source = source.to_string();
        Ok(())
    }

    /// Writes the answers into the file and returns what the file held just
    /// before, for [`Session::restore`].
    ///
    /// The file is read again first: the agent or the user may have edited it
    /// while the window was open, and writing back over the copy loaded at start
    /// would silently undo that. Answers are matched to questions by id, so they
    /// land in the right place even if blocks moved. The content returned is
    /// that same read, so a restore keeps those edits too.
    pub fn save(&mut self, answers: &AnswerSet) -> io::Result<String> {
        let current = std::fs::read_to_string(&self.path)?;
        let document = parse(&current);
        let output = write_back(&current, &document, answers);
        std::fs::write(&self.path, &output)?;
        self.document = parse(&output);
        self.source = output;
        Ok(current)
    }
}
