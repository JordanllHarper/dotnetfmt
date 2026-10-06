//! Reads and resolves the subset of `.editorconfig` settings this
//! formatter cares about: `indent_style`, `indent_size`, and `tab_width`.
//!
//! Scope for this step:
//!   - The `.editorconfig` file is assumed to live in the current working
//!     directory (no directory-walk/cascade to look further up yet).
//!   - We only resolve properties from a *single* section whose glob
//!     pattern matches C# files (e.g. `[*.cs]` or `[*]`). If zero or more
//!     than one section matches, or if any of the three required
//!     properties is missing from that section, we return an error the
//!     caller can display to the user instead of guessing.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::path::Path;

/// A single `[glob]` section of an `.editorconfig` file, with its
/// properties lowercased (both keys and values) for case-insensitive
/// lookup.
#[derive(Debug, Clone)]
struct Section {
    glob: String,
    properties: HashMap<String, String>,
}

/// How indentation is written: with the space character, or the tab
/// character.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentStyle {
    Space,
    Tab,
}

/// The resolved `indent_size` value. EditorConfig allows this to be an
/// explicit column count, or the literal value `tab`, meaning "use
/// whatever `tab_width` is".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IndentSize {
    Columns(u32),
    UseTabWidth,
}

/// The formatting-relevant settings resolved from `.editorconfig`.
#[derive(Debug, Clone)]
pub struct EditorConfig {
    pub indent_style: IndentStyle,
    pub indent_size: IndentSize,
    pub tab_width: u32,
}

/// Everything that can go wrong reading/resolving `.editorconfig`, phrased
/// so it can be printed straight to the user.
#[derive(Debug)]
pub enum EditorConfigError {
    NotFound(std::path::PathBuf),
    Io {
        path: std::path::PathBuf,
        source: std::io::Error,
    },
    GlobDoesntMatchCs,
    InvalidNumberOfSections(usize),
    MissingProperties {
        section: String,
        missing: Vec<&'static str>,
    },
    InvalidValue {
        property: &'static str,
        value: String,
        reason: String,
    },
}

impl fmt::Display for EditorConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EditorConfigError::NotFound(path) => {
                write!(f, "no .editorconfig file found at {}", path.display())
            }
            EditorConfigError::Io { path, source } => {
                write!(f, "failed to read {}: {}", path.display(), source)
            }
            EditorConfigError::GlobDoesntMatchCs => write!(
                f,
                "no section in .editorconfig matches C# files (expected something like [*.cs] or [*])"
            ),
            EditorConfigError::InvalidNumberOfSections(found) => write!(
                f,
                "expected exactly 1 section in .editorconfig, but found {found}. It must apply to C# files (e.g. [*.cs])"
            ),
            EditorConfigError::MissingProperties { section, missing } => write!(
                f,
                "section [{}] in .editorconfig is missing required propert{}: {}",
                section,
                if missing.len() == 1 { "y" } else { "ies" },
                missing.join(", ")
            ),
            EditorConfigError::InvalidValue {
                property,
                value,
                reason,
            } => write!(
                f,
                "invalid value \"{}\" for {} in .editorconfig: {}",
                value, property, reason
            ),
        }
    }
}

impl std::error::Error for EditorConfigError {}

/// Reads `.editorconfig` from `dir` and resolves the properties this
/// formatter needs.
pub fn read(dir: &Path) -> Result<EditorConfig, EditorConfigError> {
    let path = dir.join(".editorconfig");
    if !path.is_file() {
        return Err(EditorConfigError::NotFound(path));
    }
    let content = fs::read_to_string(&path).map_err(|source| EditorConfigError::Io {
        path: path.clone(),
        source,
    })?;

    let sections = parse_sections(&content);
    let section = verify_section_len(&sections)?;
    resolve(section)
}

fn verify_section_len(sections: &[Section]) -> Result<&Section, EditorConfigError> {
    if let [section] = sections {
        Ok(section)
    } else {
        Err(EditorConfigError::InvalidNumberOfSections(sections.len()))
    }
}

/// Parses raw `.editorconfig` text into a list of sections. Anything
/// before the first `[glob]` header (e.g. a top-level `root = true`) is
/// discarded, since it isn't tied to any glob and isn't needed here.
fn parse_sections(content: &str) -> Vec<Section> {
    let mut sections: Vec<Section> = Vec::new();

    for raw_line in content.lines() {
        let line = raw_line.trim();

        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }

        if let Some(glob) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            sections.push(Section {
                glob: glob.trim().to_string(),
                properties: HashMap::new(),
            });
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            // Not a section header, comment, or key=value pair; skip
            // rather than fail the whole file.
            continue;
        };

        if let Some(section) = sections.last_mut() {
            section
                .properties
                .insert(key.trim().to_lowercase(), value.trim().to_lowercase());
        }
        // Key/value pairs before any section header are dropped (see
        // doc comment above).
    }

    sections
}

/// Whether an `.editorconfig` glob pattern applies to `.cs` files.
///
/// This is intentionally not a full EditorConfig glob engine (no `**`,
/// `?`, `[seq]`, nested braces, etc.) -- just enough to recognize the
/// patterns a project's `.editorconfig` realistically uses to target C#:
/// `*`, `*.cs`, and brace lists like `*.{cs,vb}`.
fn glob_matches_cs(glob: &str) -> bool {
    let glob = glob.trim();

    if glob == "*" || glob.eq_ignore_ascii_case("*.cs") {
        return true;
    }

    if let Some(inner) = glob.strip_prefix("*.{").and_then(|s| s.strip_suffix('}')) {
        return inner
            .split(',')
            .any(|ext| ext.trim().eq_ignore_ascii_case("cs"));
    }

    false
}

const REQUIRED_PROPERTIES: [&str; 3] = ["indent_style", "indent_size", "tab_width"];

fn resolve(section: &Section) -> Result<EditorConfig, EditorConfigError> {
    if !glob_matches_cs(&section.glob) {
        return Err(EditorConfigError::GlobDoesntMatchCs);
    }

    let missing: Vec<&'static str> = REQUIRED_PROPERTIES
        .iter()
        .filter(|key| !section.properties.contains_key(**key))
        .copied()
        .collect();

    if !missing.is_empty() {
        return Err(EditorConfigError::MissingProperties {
            section: section.glob.clone(),
            missing,
        });
    }

    let indent_style = match section.properties["indent_style"].as_str() {
        "space" => IndentStyle::Space,
        "tab" => IndentStyle::Tab,
        other => {
            return Err(EditorConfigError::InvalidValue {
                property: "indent_style",
                value: other.to_string(),
                reason: "expected \"space\" or \"tab\"".to_string(),
            });
        }
    };

    let indent_size = match section.properties["indent_size"].as_str() {
        "tab" => IndentSize::UseTabWidth,
        other => match other.parse::<u32>() {
            Ok(n) if n > 0 => IndentSize::Columns(n),
            _ => {
                return Err(EditorConfigError::InvalidValue {
                    property: "indent_size",
                    value: other.to_string(),
                    reason: "expected a positive integer or \"tab\"".to_string(),
                });
            }
        },
    };

    let tab_width = match section.properties["tab_width"].parse::<u32>() {
        Ok(n) if n > 0 => n,
        _ => {
            return Err(EditorConfigError::InvalidValue {
                property: "tab_width",
                value: section.properties["tab_width"].clone(),
                reason: "expected a positive integer".to_string(),
            });
        }
    };

    Ok(EditorConfig {
        indent_style,
        indent_size,
        tab_width,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_matching_section_with_all_properties() {
        let content = "\
root = true

[*.cs]
indent_style = space
indent_size = 4
tab_width = 4
";
        let sections = parse_sections(content);
        let first = sections.first().expect("should have only 1 section");
        let config = resolve(first).expect("should resolve");
        assert_eq!(config.indent_style, IndentStyle::Space);
        assert_eq!(config.indent_size, IndentSize::Columns(4));
        assert_eq!(config.tab_width, 4);
    }

    #[test]
    fn reports_missing_properties() {
        let content = "[*.cs]\nindent_style = tab\n";
        let sections = parse_sections(content);
        let first = sections.first().expect("should have only 1 section");
        let err = resolve(first).unwrap_err();
        match err {
            EditorConfigError::MissingProperties { missing, .. } => {
                assert_eq!(missing, vec!["indent_size", "tab_width"]);
            }
            other => panic!("expected MissingProperties, got {other:?}"),
        }
    }

    #[test]
    fn reports_no_matching_section() {
        let content = "[*.txt]\nindent_style = space\nindent_size = 4\ntab_width = 4\n";
        let sections = parse_sections(content);
        let first = sections.first().expect("should have only 1 section");
        assert!(matches!(
            resolve(first),
            Err(EditorConfigError::GlobDoesntMatchCs)
        ));
    }

    #[test]
    fn reports_ambiguous_sections() {
        let content = "\
[*]
indent_style = space
indent_size = 4
tab_width = 4

[*.cs]
indent_style = tab
indent_size = 4
tab_width = 4
";
        let sections = parse_sections(content);
        assert!(matches!(
            verify_section_len(&sections),
            Err(EditorConfigError::InvalidNumberOfSections(2))
        ));
    }

    #[test]
    fn indent_size_tab_is_supported() {
        let content = "[*.cs]\nindent_style = tab\nindent_size = tab\ntab_width = 8\n";
        let sections = parse_sections(content);
        let first = sections.first().expect("there should be a value");
        let config = resolve(first).expect("should resolve");
        assert_eq!(config.indent_size, IndentSize::UseTabWidth);
    }
}
