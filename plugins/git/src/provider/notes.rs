//! Git notes view — writable user notes on commits.

use std::str::from_utf8;

use color_eyre::eyre::{Result, WrapErr};
use nyne::dispatch::context::RequestContext;
use nyne::node::{Writable, WriteOutcome};

/// Default cap on notes entries scanned per file.
const NOTES_LIMIT: usize = 50;

git_template_view!(
    #[derive(Clone)]
    NotesView,
    |repo, path| repo.file_notes(path, NOTES_LIMIT)
);

/// [`Writable`] implementation for `NotesView` — sets or removes a git note.
impl Writable for NotesView {
    /// Writes a git note to the file.
    fn write(&self, _ctx: &RequestContext<'_>, data: &[u8]) -> Result<WriteOutcome> {
        let message = from_utf8(data).wrap_err("note content must be valid UTF-8")?;
        self.0.repo.set_note(&self.0.rel_path, message)?;
        Ok(WriteOutcome::Written(data.len()))
    }
}
