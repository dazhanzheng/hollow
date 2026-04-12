//! JupyterExtractor: extracts source text from Jupyter notebooks (.ipynb).
//!
//! An .ipynb file is JSON. For full-text search indexing we only care about
//! the `source` field of `code` and `markdown` cells — the `outputs` field
//! on code cells can contain base64-encoded raster images (matplotlib
//! figures, HTML tables with embedded images, etc.) that need OCR or HTML
//! handling we don't want to run here.
//!
//! Cell sources are either a single string or an array of strings (one per
//! line). Both shapes are normalised to a single `String`.

use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::content::extractor::{ExtractionError, ExtractionResult, Extractor};
use crate::content::extractors::common::DEFAULT_MAX_FILE_SIZE;

pub struct JupyterExtractor {
    max_size: u64,
}

impl JupyterExtractor {
    pub fn new() -> Self {
        Self {
            max_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

impl Default for JupyterExtractor {
    fn default() -> Self {
        Self::new()
    }
}

const SUPPORTED: &[&str] = &["application/x-ipynb+json"];

impl Extractor for JupyterExtractor {
    fn name(&self) -> &'static str {
        "Jupyter"
    }

    fn supported_mimes(&self) -> &[&'static str] {
        SUPPORTED
    }

    fn extract(&self, path: &Path) -> Result<ExtractionResult, ExtractionError> {
        let metadata = fs::metadata(path)?;
        let size = metadata.len();
        if size > self.max_size {
            return Err(ExtractionError::FileTooLarge {
                size,
                limit: self.max_size,
            });
        }

        let bytes = fs::read(path)?;
        let notebook: Value = serde_json::from_slice(&bytes)
            .map_err(|e| ExtractionError::Io(std::io::Error::other(
                format!("ipynb parse: {}", e)
            )))?;

        let body_text = extract_cell_sources(&notebook);

        Ok(ExtractionResult {
            body_text,
            encoding: Some("UTF-8".to_string()),
        })
    }
}

/// Walk the notebook's `cells` array and collect `source` from every cell
/// whose `cell_type` is `code` or `markdown`. Raw cells are also included —
/// they're plain text by definition. Outputs are ignored.
fn extract_cell_sources(notebook: &Value) -> String {
    let mut out = String::new();

    let Some(cells) = notebook.get("cells").and_then(|v| v.as_array()) else {
        return out;
    };

    for cell in cells {
        let cell_type = cell
            .get("cell_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        // Skip unknown cell types defensively.
        if !matches!(cell_type, "code" | "markdown" | "raw") {
            continue;
        }

        let Some(source) = cell.get("source") else {
            continue;
        };

        let text = join_source(source);
        if !text.trim().is_empty() {
            out.push_str(&text);
            // Cell separator — blank line, so downstream tokenization
            // doesn't accidentally merge the end of one cell with the
            // start of the next.
            if !out.ends_with('\n') {
                out.push('\n');
            }
            out.push('\n');
        }
    }

    out
}

/// A cell `source` is either a JSON string or an array of strings (one
/// per line, including trailing newlines). Normalize both to a single
/// `String`.
fn join_source(source: &Value) -> String {
    match source {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| p.as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_ipynb_test");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn test_extract_mixed_cells() {
        // Note: use br##"..."## rather than br#"..."# because the JSON body
        // contains the literal sequence `"#` (inside `"# Analysis\n"`) which
        // would otherwise close a single-# raw byte string early.
        let ipynb = br##"{
          "cells": [
            {
              "cell_type": "markdown",
              "source": ["# Analysis\n", "\n", "Loading data...\n"]
            },
            {
              "cell_type": "code",
              "source": "import pandas as pd\ndf = pd.read_csv('data.csv')",
              "outputs": []
            }
          ],
          "metadata": {},
          "nbformat": 4,
          "nbformat_minor": 5
        }"##;
        let p = tmp("nb1.ipynb", ipynb);
        let e = JupyterExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("# Analysis"));
        assert!(r.body_text.contains("Loading data"));
        assert!(r.body_text.contains("import pandas"));
        assert!(r.body_text.contains("read_csv"));
    }

    #[test]
    fn test_ignores_output_cells() {
        // A notebook where a code cell has an image output — we must NOT
        // include that base64 blob in the extracted text.
        let ipynb = br#"{
          "cells": [
            {
              "cell_type": "code",
              "source": ["plt.plot([1,2,3])\n"],
              "outputs": [
                {
                  "output_type": "display_data",
                  "data": {
                    "image/png": "AAAABBBBCCCCDDDDEEEEFFFFGGGG_base64_blob"
                  }
                }
              ]
            }
          ],
          "metadata": {},
          "nbformat": 4,
          "nbformat_minor": 5
        }"#;
        let p = tmp("nb2.ipynb", ipynb);
        let e = JupyterExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("plt.plot"));
        assert!(!r.body_text.contains("AAAABBBB"));
        assert!(!r.body_text.contains("image/png"));
    }

    #[test]
    fn test_source_as_string() {
        // Some tools write source as a single string instead of a line array.
        let ipynb = br#"{
          "cells": [
            {
              "cell_type": "code",
              "source": "print('hello world')",
              "outputs": []
            }
          ]
        }"#;
        let p = tmp("nb3.ipynb", ipynb);
        let e = JupyterExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("hello world"));
    }

    #[test]
    fn test_empty_notebook() {
        let ipynb = br#"{"cells": [], "metadata": {}}"#;
        let p = tmp("empty.ipynb", ipynb);
        let e = JupyterExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.trim().is_empty());
    }

    #[test]
    fn test_invalid_json_errors() {
        let p = tmp("broken.ipynb", b"not json at all");
        let e = JupyterExtractor::new();
        assert!(e.extract(&p).is_err());
    }

    #[test]
    fn test_utf8_in_source() {
        let ipynb = r##"{
          "cells": [
            {"cell_type": "markdown", "source": ["# 你好世界\n"]}
          ]
        }"##
        .as_bytes();
        let p = tmp("zh.ipynb", ipynb);
        let e = JupyterExtractor::new();
        let r = e.extract(&p).unwrap();
        assert!(r.body_text.contains("你好世界"));
    }
}
