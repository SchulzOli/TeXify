//! Error types.

use std::fmt;

/// User-facing error (bad input, missing template, compile failure, ...).
#[derive(Debug)]
pub enum Error {
    Msg(String),
    Compile(CompileError),
}

/// A LaTeX run failed; carries the relevant log excerpt and source lines.
#[derive(Debug, Clone)]
pub struct CompileError {
    pub message: String,
    pub log: String,
    pub source: String,
}

impl CompileError {
    pub fn details(&self) -> String {
        let mut parts = vec![self.message.clone()];
        if !self.log.is_empty() {
            parts.extend(["".into(), "--- LaTeX log ---".into(), self.log.clone()]);
        }
        if !self.source.is_empty() {
            parts.extend(["".into(), "--- Source at the error ---".into(), self.source.clone()]);
        }
        parts.join("\n")
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Msg(m) => f.write_str(m),
            Error::Compile(c) => f.write_str(&c.message),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// `err!("...")` builds an [`Error::Msg`] like `format!`.
#[macro_export]
macro_rules! err {
    ($($arg:tt)*) => { $crate::error::Error::Msg(format!($($arg)*)) };
}

/// `bail!("...")` returns early with an [`Error::Msg`].
#[macro_export]
macro_rules! bail {
    ($($arg:tt)*) => { return Err($crate::err!($($arg)*)) };
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Msg(e.to_string())
    }
}

impl From<minijinja::Error> for Error {
    fn from(e: minijinja::Error) -> Self {
        Error::Msg(e.to_string())
    }
}
