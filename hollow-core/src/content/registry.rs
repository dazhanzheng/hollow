//! ExtractorRegistry: maps MIME types (and extensions) to Extractor implementations.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};

use crate::content::extractor::Extractor;
use crate::content::extractors::docx::DocxExtractor;
use crate::content::extractors::epub::EpubExtractor;
use crate::content::extractors::fb2::Fb2Extractor;
use crate::content::extractors::html::HtmlExtractor;
use crate::content::extractors::jupyter::JupyterExtractor;
use crate::content::extractors::plain_text::PlainTextExtractor;
use crate::content::extractors::rtf::RtfExtractor;
use crate::content::extractors::source_code::SourceCodeExtractor;
use crate::content::extractors::svg::SvgExtractor;

/// Static descriptor for a plugin — used for the settings UI.
#[derive(Debug, Clone)]
pub struct PluginDescriptor {
    pub name: &'static str,
    pub display_name: &'static str,
    pub description: &'static str,
    pub extensions: &'static [&'static str],
}

/// All plugins shipped with hollow. Add a row here when registering a new extractor.
pub fn plugin_descriptors() -> Vec<PluginDescriptor> {
    vec![
        PluginDescriptor {
            name: "PlainText",
            display_name: "Plain Text",
            description: "Plain text, Markdown, CSV/TSV, JSON/YAML/TOML, XML, RSS/Atom, iCalendar, vCard, email, subtitles, config files, and more.",
            extensions: PlainTextExtractor::known_extensions(),
        },
        PluginDescriptor {
            name: "SourceCode",
            display_name: "Source Code",
            description: "Source files for programming languages and infrastructure/build DSLs (Python, Rust, Swift, JS/TS, Go, Java, C/C++, shell scripts, Terraform, Dockerfile, Makefile, …).",
            extensions: SourceCodeExtractor::known_extensions(),
        },
        PluginDescriptor {
            name: "Html",
            display_name: "HTML",
            description: "Web pages and saved HTML documents. Strips tags and scripts, keeping readable text for search.",
            extensions: &["html", "htm", "xhtml"],
        },
        PluginDescriptor {
            name: "Docx",
            display_name: "Word (.docx)",
            description: "Microsoft Word Open XML documents. Extracts body text from paragraphs and runs.",
            extensions: &["docx"],
        },
        PluginDescriptor {
            name: "Rtf",
            display_name: "Rich Text (.rtf)",
            description: "Rich Text Format documents. Extracts plain text while discarding formatting.",
            extensions: &["rtf"],
        },
        PluginDescriptor {
            name: "Epub",
            display_name: "EPUB",
            description: "EPUB ebooks. Concatenates readable text from all XHTML chapters for search indexing.",
            extensions: &["epub"],
        },
        PluginDescriptor {
            name: "Svg",
            display_name: "SVG",
            description: "Scalable Vector Graphics. Extracts text labels, titles, and descriptions — geometry and raster <image> elements are ignored.",
            extensions: &["svg", "svgz"],
        },
        PluginDescriptor {
            name: "Jupyter",
            display_name: "Jupyter Notebook",
            description: "Jupyter .ipynb notebooks. Extracts source from code, markdown, and raw cells. Output cells (which may contain embedded images) are skipped.",
            extensions: &["ipynb"],
        },
        PluginDescriptor {
            name: "Fb2",
            display_name: "FictionBook (.fb2)",
            description: "FictionBook 2 ebooks. Streams out all paragraph and title text, skipping embedded binary image sections.",
            extensions: &["fb2"],
        },
    ]
}

/// Process-global set of disabled extractor names. Populated from the Swift
/// settings layer at startup and on toggle changes. Checked by the pipeline
/// before dispatching extraction.
fn disabled_set() -> &'static Mutex<HashSet<String>> {
    static INSTANCE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    INSTANCE.get_or_init(|| Mutex::new(HashSet::new()))
}

pub fn is_extractor_disabled(name: &str) -> bool {
    disabled_set()
        .lock()
        .map(|s| s.contains(name))
        .unwrap_or(false)
}

pub fn set_extractor_enabled(name: &str, enabled: bool) {
    if let Ok(mut set) = disabled_set().lock() {
        if enabled {
            set.remove(name);
        } else {
            set.insert(name.to_string());
        }
    }
}

pub struct ExtractorRegistry {
    by_mime: HashMap<String, Arc<dyn Extractor>>,
    by_extension: HashMap<String, Arc<dyn Extractor>>,
    /// Exact-filename lookup, used for no-extension files like `Dockerfile`,
    /// `Makefile`, `Rakefile`, `Gemfile`. Keys are compared case-insensitively.
    by_basename: HashMap<String, Arc<dyn Extractor>>,
}

impl ExtractorRegistry {
    pub fn new() -> Self {
        Self {
            by_mime: HashMap::new(),
            by_extension: HashMap::new(),
            by_basename: HashMap::new(),
        }
    }

    pub fn register(&mut self, extractor: Arc<dyn Extractor>) {
        for mime in extractor.supported_mimes() {
            self.by_mime
                .insert(mime.to_string(), Arc::clone(&extractor));
        }
    }

    pub fn register_with_extensions(
        &mut self,
        extractor: Arc<dyn Extractor>,
        extensions: &[&str],
    ) {
        self.register(Arc::clone(&extractor));
        for ext in extensions {
            self.by_extension
                .insert(ext.to_lowercase(), Arc::clone(&extractor));
        }
    }

    /// Register an extractor against specific filenames (no extension). Used
    /// for the `Dockerfile` / `Makefile` / `Rakefile` family.
    pub fn register_with_basenames(
        &mut self,
        extractor: Arc<dyn Extractor>,
        basenames: &[&str],
    ) {
        // Don't call `register()` here — the extractor's MIME list has
        // already been wired up elsewhere if applicable. We just want to
        // attach basename routes.
        for name in basenames {
            self.by_basename
                .insert(name.to_lowercase(), Arc::clone(&extractor));
        }
    }

    pub fn find_by_mime(&self, mime: &str) -> Option<Arc<dyn Extractor>> {
        self.by_mime.get(mime).cloned()
    }

    pub fn find_by_extension(&self, ext: &str) -> Option<Arc<dyn Extractor>> {
        self.by_extension.get(&ext.to_lowercase()).cloned()
    }

    pub fn find_by_basename(&self, basename: &str) -> Option<Arc<dyn Extractor>> {
        self.by_basename.get(&basename.to_lowercase()).cloned()
    }
}

impl Default for ExtractorRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Default registry with all built-in extractors registered.
pub fn default_registry() -> ExtractorRegistry {
    let mut r = ExtractorRegistry::new();

    // PlainText — the broadest extractor. Wired up by MIME (catch-all for
    // the text/plain heuristic fallback), by extension (every text-ish
    // format we know about), and by basename (no-extension config files).
    let plain_text = Arc::new(PlainTextExtractor::new());
    r.register_with_extensions(
        Arc::clone(&plain_text) as Arc<dyn Extractor>,
        PlainTextExtractor::known_extensions(),
    );
    r.register_with_basenames(
        Arc::clone(&plain_text) as Arc<dyn Extractor>,
        PlainTextExtractor::known_basenames(),
    );

    // SourceCode — programming languages + build-script filenames.
    let source_code = Arc::new(SourceCodeExtractor::new());
    r.register_with_extensions(
        Arc::clone(&source_code) as Arc<dyn Extractor>,
        SourceCodeExtractor::known_extensions(),
    );
    r.register_with_basenames(
        Arc::clone(&source_code) as Arc<dyn Extractor>,
        SourceCodeExtractor::known_basenames(),
    );

    r.register_with_extensions(
        Arc::new(HtmlExtractor::new()),
        &["html", "htm", "xhtml"],
    );
    r.register_with_extensions(
        Arc::new(DocxExtractor::new()),
        &["docx"],
    );
    r.register_with_extensions(
        Arc::new(RtfExtractor::new()),
        &["rtf"],
    );
    r.register_with_extensions(
        Arc::new(EpubExtractor::new()),
        &["epub"],
    );
    r.register_with_extensions(
        Arc::new(SvgExtractor::new()),
        &["svg", "svgz"],
    );
    r.register_with_extensions(
        Arc::new(JupyterExtractor::new()),
        &["ipynb"],
    );
    r.register_with_extensions(
        Arc::new(Fb2Extractor::new()),
        &["fb2"],
    );
    r
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_registry_finds_plain_text() {
        let r = default_registry();
        let e = r.find_by_mime("text/plain").unwrap();
        assert_eq!(e.name(), "PlainText");
    }

    #[test]
    fn test_default_registry_finds_source_code_by_mime() {
        let r = default_registry();
        let e = r.find_by_mime("text/x-rust").unwrap();
        assert_eq!(e.name(), "SourceCode");
    }

    #[test]
    fn test_default_registry_finds_source_by_extension() {
        let r = default_registry();
        let e = r.find_by_extension("py").unwrap();
        assert_eq!(e.name(), "SourceCode");
        let e = r.find_by_extension("RS").unwrap(); // case insensitive
        assert_eq!(e.name(), "SourceCode");
    }

    #[test]
    fn test_unknown_mime_returns_none() {
        let r = default_registry();
        assert!(r.find_by_mime("image/png").is_none());
    }

    #[test]
    fn test_plain_text_registered_by_new_extensions() {
        let r = default_registry();
        // Tier 1 extensions should route to PlainText.
        for ext in ["rst", "ics", "vcf", "eml", "srt", "ini", "proto", "xsd", "gpx"] {
            let e = r
                .find_by_extension(ext)
                .unwrap_or_else(|| panic!("missing extension registration for .{}", ext));
            assert_eq!(e.name(), "PlainText", "wrong extractor for .{}", ext);
        }
    }

    #[test]
    fn test_source_code_registered_by_new_extensions() {
        let r = default_registry();
        // Tier 2 extensions should route to SourceCode.
        for ext in ["ps1", "nix", "tf", "sol", "zig", "jl", "fs", "asm", "ml"] {
            let e = r
                .find_by_extension(ext)
                .unwrap_or_else(|| panic!("missing extension registration for .{}", ext));
            assert_eq!(e.name(), "SourceCode", "wrong extractor for .{}", ext);
        }
    }

    #[test]
    fn test_basename_routes_dockerfile_and_makefile_to_source_code() {
        let r = default_registry();
        let dockerfile = r.find_by_basename("Dockerfile").unwrap();
        assert_eq!(dockerfile.name(), "SourceCode");

        let makefile = r.find_by_basename("Makefile").unwrap();
        assert_eq!(makefile.name(), "SourceCode");

        // Case insensitive
        let lowercase = r.find_by_basename("makefile").unwrap();
        assert_eq!(lowercase.name(), "SourceCode");
    }

    #[test]
    fn test_basename_routes_dotfiles_to_plain_text() {
        let r = default_registry();
        for name in [".gitignore", ".editorconfig", ".bashrc", "README"] {
            let e = r
                .find_by_basename(name)
                .unwrap_or_else(|| panic!("missing basename registration for {}", name));
            assert_eq!(e.name(), "PlainText", "wrong extractor for {}", name);
        }
    }

    #[test]
    fn test_unknown_basename_returns_none() {
        let r = default_registry();
        assert!(r.find_by_basename("totally-made-up-file").is_none());
    }

    #[test]
    fn test_plugin_descriptor_extensions_stay_in_sync() {
        // plugin_descriptors() is the source of truth for the UI; the actual
        // registration is the source of truth for routing. Verify they agree
        // for the two extensible plugins.
        let descriptors = plugin_descriptors();
        let plain = descriptors.iter().find(|d| d.name == "PlainText").unwrap();
        assert!(
            plain.extensions.contains(&"rst"),
            "PluginDescriptor extensions should include new Tier 1 entries"
        );
        let source = descriptors.iter().find(|d| d.name == "SourceCode").unwrap();
        assert!(
            source.extensions.contains(&"nix"),
            "PluginDescriptor extensions should include new Tier 2 entries"
        );
    }
}
