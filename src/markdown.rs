//! Markdown processing
use crate::{Error, Result, TomlMap};
use serde::{de::DeserializeOwned, Serialize};
use toml::value::Value;

/// tokens to indicate frontmatter metadata
pub(crate) const TOML_START: &str = "+++\n";
pub(crate) const TOML_END: &str = "\n+++\n";
pub(crate) const YAML_START: &str = "---\n";
pub(crate) const YAML_END: &str = "\n---\n";

#[derive(Debug, PartialEq)]
pub enum Frontmatter<'md> {
    Toml(&'md str),
    Yaml(&'md str),
    Empty,
}

impl<'md> Frontmatter<'md> {
    /// Returns true if the frontmatter is empty
    pub fn is_empty(&self) -> bool {
        matches!(self, Self::Empty)
    }
    /// Parses frontmatter into object T, or returns Error::FrontmatterParse
    pub fn parse<T: DeserializeOwned>(&self) -> Result<T> {
        match self {
            Self::Toml(buf) => {
                Ok(toml::from_str(buf).map_err(|e| Error::FrontmatterParse(e.to_string()))?)
            }
            Self::Yaml(buf) => Ok(
                serde_yaml::from_str(buf).map_err(|e| Error::FrontmatterParse(e.to_string()))?
            ),
            Self::Empty => Err(Error::FrontmatterParse("no content".into())),
        }
    }
}

/// Split markdown file into Frontmatter and content.
/// Both have leading and trailing whitespace removed
pub fn split_markdown(markdown: &str) -> (Frontmatter, &str) {
    if markdown.starts_with(TOML_START) {
        let (front, body) = remove_frontmatter(markdown, TOML_START, TOML_END);
        let front = if !front.is_empty() {
            Frontmatter::Toml(front)
        } else {
            Frontmatter::Empty
        };
        (front, body)
    } else if markdown.starts_with(YAML_START) {
        let (front, body) = remove_frontmatter(markdown, YAML_START, YAML_END);
        let front = if !front.is_empty() {
            Frontmatter::Yaml(front)
        } else {
            Frontmatter::Empty
        };
        (front, body)
    } else {
        (Frontmatter::Empty, markdown)
    }
}

/// Parse frontmatter to known data structure.
pub fn parse_frontmatter<T: DeserializeOwned>(front: Frontmatter) -> Result<T> {
    match front {
        Frontmatter::Toml(data) => {
            let ghd = toml::from_str(data).map_err(|e| Error::FrontmatterParse(e.to_string()))?;
            Ok(ghd)
        }
        Frontmatter::Yaml(data) => {
            let ghd =
                serde_yaml::from_str(data).map_err(|e| Error::FrontmatterParse(e.to_string()))?;
            Ok(ghd)
        }
        Frontmatter::Empty => Err(Error::FrontmatterParse(
            "markdown file is missing header".into(),
        )),
    }
}

/// Parse frontmatter to known data structure.
pub fn parse_frontmatter_to_map(front: Frontmatter) -> Result<TomlMap> {
    match front {
        Frontmatter::Toml(data) => {
            match data
                .parse::<Value>()
                .map_err(|e| Error::FrontmatterParse(e.to_string()))?
            {
                Value::Table(table) => Ok(table),
                _ => Err(Error::FrontmatterParse("Expected toml values".to_string())),
            }
        }
        Frontmatter::Yaml(data) => {
            // does this work?
            let ghd =
                serde_yaml::from_str(data).map_err(|e| Error::FrontmatterParse(e.to_string()))?;
            Ok(ghd)
        }
        Frontmatter::Empty => Err(Error::FrontmatterParse(
            "markdown file is missing header".into(),
        )),
    }
}

/// Split the markdown file into header and body strings based on start/end tags
/// Both strings have leading and trailing whitespace removed
fn remove_frontmatter<'md>(
    markdown: &'md str,
    start: &'_ str,
    end: &'_ str,
) -> (&'md str, &'md str) {
    if markdown.starts_with(start) {
        // to allow "+++\n+++\n" for empty frontmatter, subtract one from start index
        let rest = &markdown[start.len() - 1..];
        if let Some(end_ix) = rest.find(end) {
            let front = (&rest[..end_ix]).trim();
            let back = (&rest[end_ix + end.len()..]).trim();
            return (front, back);
        }
    }
    ("", markdown)
}

/// Convert markdown header metadata to toml header (with +++ prefix/suffix)
fn make_toml_frontmatter<T: Serialize>(data: &T) -> Result<String> {
    Ok(format!(
        "{}{}{}",
        TOML_START,
        toml::to_string(data)?,
        TOML_END
    ))
}

/// Writes toml metadata + content markdown to output file
pub fn write_markdown<T: Serialize, W: std::io::Write>(
    data: &T,
    content: &str,
    writer: &mut W,
) -> Result<()> {
    let toml_header = make_toml_frontmatter(data)?;
    writer.write_all(toml_header.as_bytes())?;
    writer.write_all(content.as_bytes())?;
    writer.flush()?;
    Ok(())
}

#[test]
fn split_toml() {
    use crate::markdown::{split_markdown, Frontmatter};

    let (front, body) = split_markdown("+++\nthing = \"one\"\n+++\nhello");
    assert_eq!(front, Frontmatter::Toml("thing = \"one\""));
    assert_eq!(body, "hello");

    // empty Toml frontmatter
    let (front, body) = split_markdown("+++\n+++\nhello");
    assert_eq!(front, Frontmatter::Empty);
    assert_eq!(body, "hello");

    // missing body
    let (front, body) = split_markdown("+++\nthing = \"one\"\n+++\n");
    assert_eq!(front, Frontmatter::Toml("thing = \"one\""));
    assert_eq!(body, "");

    // no end tag: not toml
    let (front, body) = split_markdown("+++\nhello");
    assert_eq!(front, Frontmatter::Empty);
    assert_eq!(body, "+++\nhello");

    // invalid start tag
    let (front, body) = split_markdown("++++\n+++\nhello");
    assert_eq!(front, Frontmatter::Empty);
    assert_eq!(body, "++++\n+++\nhello");

    // missing body
    let (front, body) = split_markdown("+++\nthing = \"one\"\n+++\n");
    assert_eq!(front, Frontmatter::Toml("thing = \"one\""));
    assert_eq!(body, "");

    // trim whitespace at ends of strings
    let (front, body) = split_markdown("+++\n   \n+++\n\n  hello  \n");
    assert_eq!(front, Frontmatter::Empty);
    assert_eq!(body, "hello");
}

#[test]
fn split_yaml() {
    use crate::markdown::{split_markdown, Frontmatter};

    let (front, body) = split_markdown("---\nthing: one\n---\nhello");
    assert_eq!(front, Frontmatter::Yaml("thing: one"));
    assert_eq!(body, "hello");

    // empty Yaml frontmatter
    let (front, body) = split_markdown("---\n---\nhello");
    assert_eq!(front, Frontmatter::Empty);
    assert_eq!(body, "hello");

    // missing body
    let (front, body) = split_markdown("---\nthing: one\n---\n");
    assert_eq!(front, Frontmatter::Yaml("thing: one"));
    assert_eq!(body, "");

    // trim whitespace
    let (front, body) = split_markdown("---\n   \n---\n\n  hello  \n");
    assert_eq!(front, Frontmatter::Empty);
    assert_eq!(body, "hello");
}

#[test]
fn test_split() {
    use crate::markdown::{split_markdown, Frontmatter};

    // missing frontmatter
    let (front, body) = split_markdown("hello");
    assert_eq!(front, Frontmatter::Empty);
    assert_eq!(body, "hello");
}

#[test]
fn test_toml_parse() {
    // parse with comments, blank lines, and variables
    let map = parse_toml("# comment \n\nboo=\"baz\"\n# comment\ncount = 99").expect("parsed");
    assert_eq!(map.get("boo"), Some(Value::from("baz")).as_ref());
    assert_eq!(map.get("count"), Some(Value::from(99)).as_ref());
}
