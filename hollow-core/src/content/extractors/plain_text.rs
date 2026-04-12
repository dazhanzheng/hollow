//! PlainTextExtractor: handles plain text, markdown, CSV, JSON, YAML, etc.

use std::path::Path;

use crate::content::extractor::{ExtractionError, ExtractionResult, Extractor};
use crate::content::extractors::common::{read_text_file, DEFAULT_MAX_FILE_SIZE};

pub struct PlainTextExtractor {
    max_size: u64,
}

impl PlainTextExtractor {
    pub fn new() -> Self {
        Self {
            max_size: DEFAULT_MAX_FILE_SIZE,
        }
    }
}

impl Default for PlainTextExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl PlainTextExtractor {
    /// Known file extensions this extractor should handle, used to build the
    /// registry's `by_extension` lookup table. Kept intentionally broad —
    /// anything that's "essentially UTF-8 text with no heavy binary parsing
    /// needed" lives here.
    ///
    /// Excludes source-code languages (those live on SourceCodeExtractor)
    /// and structured-document formats with dedicated extractors
    /// (html, docx, rtf, epub).
    pub fn known_extensions() -> &'static [&'static str] {
        &[
            // Basic text
            "txt", "text", "md", "markdown", "mdown", "mkd", "mkdn",
            "log", "readme",
            // Structured prose / docs
            "rst", "org", "adoc", "asciidoc", "asc", "tex", "bib", "nfo",
            "textile", "wiki",
            // Data serialization
            "csv", "tsv", "psv",
            "json", "jsonl", "ndjson", "json5", "jsonc",
            "xml", "xsd", "dtd", "xslt", "xsl",
            "yaml", "yml",
            "toml",
            "plist",
            // Feeds / syndication
            "rss", "atom", "opml",
            // Calendar / contacts / mail
            "ics", "ifb", "vcs", "vcf", "eml", "mbox",
            // Geo
            "kml", "gpx",
            // Subtitles
            "srt", "vtt", "ass", "ssa", "sub", "sbv", "lrc",
            // Config / dotfiles (by extension)
            "ini", "cfg", "conf", "config", "properties", "env",
            "editorconfig", "gitignore", "gitattributes", "dockerignore",
            "npmrc", "nvmrc", "yarnrc", "rvmrc", "lock", "bazelrc",
            "vimrc", "emacs", "tmux", "tmux.conf",
            // Patches / diffs
            "patch", "diff",
            // Data contracts
            "graphql", "gql", "proto", "thrift", "avsc",
            // i18n / localization
            "po", "pot", "strings", "xcstrings", "arb", "resx",
            // Checksums / bookmarks / shortcuts
            "md5", "sha", "sha1", "sha256", "sha512", "url", "webloc",
            // Linux desktop / systemd / launchd
            "desktop", "service", "unit", "timer", "launchd",
            // Misc
            "csv.gz", // will fall through as binary anyway, but harmless
        ]
    }

    /// Exact filenames (no extension) this extractor should claim. These
    /// are routed via the registry's basename lookup.
    ///
    /// Note: some of these (Makefile, Dockerfile) are arguably "source code"
    /// and could belong on SourceCodeExtractor instead. Both extractors run
    /// the same underlying read_text_file pipeline, so the routing is
    /// essentially cosmetic — we put build-script-ish filenames on
    /// SourceCodeExtractor (see below) and pure-config ones here.
    pub fn known_basenames() -> &'static [&'static str] {
        &[
            // Ruby bundlers / rails
            "Gemfile", "Gemfile.lock", "Rakefile",
            // Homebrew / ruby
            "Brewfile",
            // Heroku / 12-factor
            "Procfile",
            // Common no-ext configs
            ".editorconfig", ".gitignore", ".gitattributes",
            ".dockerignore", ".npmrc", ".nvmrc", ".yarnrc",
            ".env", ".envrc", ".bazelrc",
            // Shell rc files
            ".bashrc", ".zshrc", ".profile", ".bash_profile",
            ".zprofile", ".zshenv", ".inputrc",
            // Misc
            "CHANGELOG", "CHANGES", "HISTORY", "NEWS", "AUTHORS",
            "CONTRIBUTORS", "LICENSE", "COPYING", "NOTICE",
            "README", "INSTALL", "TODO",
        ]
    }
}

const SUPPORTED: &[&str] = &[
    "text/plain",
    "text/markdown",
    "text/csv",
    "text/tab-separated-values",
    "text/x-log",
    "application/json",
    "application/xml",
    "text/xml",
    "application/yaml",
    "text/yaml",
    "application/toml",
    "text/toml",
    // RSS/Atom/OPML
    "application/rss+xml",
    "application/atom+xml",
    "text/x-opml",
    // Calendar / contacts / mail
    "text/calendar",
    "text/vcard",
    "message/rfc822",
    // Geo XML
    "application/vnd.google-earth.kml+xml",
    "application/gpx+xml",
    // Subtitles
    "application/x-subrip",
    "text/vtt",
    // Patches
    "text/x-patch",
    "text/x-diff",
    // Data contracts
    "application/graphql",
    "application/x-protobuf",
];

impl Extractor for PlainTextExtractor {
    fn name(&self) -> &'static str {
        "PlainText"
    }

    fn supported_mimes(&self) -> &[&'static str] {
        SUPPORTED
    }

    fn extract(&self, path: &Path) -> Result<ExtractionResult, ExtractionError> {
        read_text_file(path, self.max_size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn tmp(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join("hollow_plain_test");
        fs::create_dir_all(&dir).unwrap();
        let p = dir.join(name);
        fs::File::create(&p).unwrap().write_all(bytes).unwrap();
        p
    }

    #[test]
    fn test_name_and_mimes() {
        let e = PlainTextExtractor::new();
        assert_eq!(e.name(), "PlainText");
        assert!(e.supported_mimes().contains(&"text/plain"));
        assert!(e.supported_mimes().contains(&"application/json"));
    }

    #[test]
    fn test_extract_utf8() {
        let p = tmp("greet.txt", "你好\nworld".as_bytes());
        let e = PlainTextExtractor::new();
        let result = e.extract(&p).unwrap();
        assert_eq!(result.body_text, "你好\nworld");
    }
}
