//! TeXify — Markdown/Text -> LaTeX -> PDF.
//!
//! LaTeX is the design layer: templates turn plain, readable Markdown (plus a
//! YAML frontmatter for structured data) into professional documents such as
//! CVs, cover letters, invoices, proposals and contracts.

pub mod ai;
pub mod build;
pub mod cli;
pub mod config;
pub mod engine;
pub mod error;
pub mod importers;
pub mod latex;
pub mod markdown;
pub mod mcp;
pub mod pdftools;
pub mod templates;
pub mod yaml;

pub use error::{CompileError, Error, Result};

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
