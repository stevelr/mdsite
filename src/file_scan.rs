//! File scanner - scan directory tree to catalog markdown files and templates.
//! This can be used either for generating an index, or for driving copy operations
//! for static site gen.

use crate::{
    markdown::{parse_frontmatter, split_markdown},
    Error, Result,
};
use ignore::{DirEntry, WalkBuilder};
use serde::de::DeserializeOwned;
use std::path::{Path, PathBuf};

const MARKDOWN_EXTENSION: &str = "md";
const HANDLEBARS_EXTENSION: &str = "hbs";

// split path into base (either copy_base or one of the content dirs) and relative
fn split(entry: &DirEntry) -> (&Path, &Path) {
    let file_base = entry.path().ancestors().nth(entry.depth()).unwrap();
    let relative_path = entry.path().strip_prefix(file_base).unwrap();
    (file_base, relative_path)
}

/// Markdown file info
pub struct MarkdownPath {
    ///  Full path to file, including source path
    pub path: PathBuf,
    /// Relative path from its source dir
    pub rel_path: PathBuf,
}

/// Markdown file info with data
pub struct MarkdownData<T: DeserializeOwned> {
    ///  Full path to file, including source path
    pub path: PathBuf,
    /// Relative path from its source dir
    pub rel_path: PathBuf,
    /// Parsed header
    pub frontmatter: Result<T>,
}

/// Results of file scan
pub struct ScanResults {
    /// All templates found
    pub templates: Vec<PathBuf>,
    /// All markdown files found
    pub markdown: Vec<MarkdownPath>,
}

/// Options for file scanner
pub struct ScanOptions {
    /// Whether to follow symbolic links (default: false)
    pub follow_links: bool,
    /// Whether to load and parse frontmatter from markdown files (default false).
    pub load_frontmatter: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            follow_links: false,
            load_frontmatter: false,
        }
    }
}

/// Collects parsed metadata from each file. If there are any errors reading the file
/// (such as file permission problems), returns an Error.
/// Does not return errors immediately if frontmatter isn't parsed correctly
/// (such as missing required fields, or other syntax errors). Each frontmatter
/// returned is a Result containing successful parsed object or an error for that file.
/// This can be used to display file-specific error messages if desired.
pub fn load_frontmatter<T: DeserializeOwned>(
    files: Vec<MarkdownPath>,
) -> Result<Vec<MarkdownData<T>>> {
    use std::fs::read_to_string;

    files
        .into_iter()
        .map(|mdp| {
            let body = read_to_string(&mdp.path)?;
            let (front, _) = split_markdown(&body);
            let frontmatter = parse_frontmatter(front);
            Ok(MarkdownData {
                path: mdp.path,
                rel_path: mdp.rel_path,
                frontmatter,
            })
        })
        .collect()
}

/// scan folders to build index of markdown and template files
pub fn index_sources(sources: Vec<PathBuf>, opt: &ScanOptions) -> Result<ScanResults> {
    let mut markdown: Vec<MarkdownPath> = Vec::new();
    let mut templates: Vec<PathBuf> = Vec::new();

    let mut walk = match sources.split_first() {
        Some((first, others)) => {
            if !first.is_dir() {
                return Err(Error::InvalidScanDir(first.display().to_string()));
            }
            let mut walk = WalkBuilder::new(first);
            for dir in others.iter() {
                if !dir.is_dir() {
                    return Err(Error::InvalidScanDir(dir.display().to_string()));
                }
                walk.add(dir);
            }
            walk
        }
        None => return Err(Error::ScanNoSources),
    };
    // enable standard ignore filters (hidden, .gitignore, .ignore, global git ignore/excludes
    walk.standard_filters(true)
        // enable ignore files from  parents of each included dir
        .parents(true)
        // whether to follow symbolic links
        .follow_links(opt.follow_links);
    for res in walk.build() {
        let entry = ok_entry(res)?;
        if !file_filter(&entry) {
            continue;
        }
        let (_, relative_path) = split(&entry);
        if let Some(ext) = entry.path().extension() {
            match ext.to_str() {
                Some(MARKDOWN_EXTENSION) => {
                    markdown.push(MarkdownPath {
                        path: entry.path().to_path_buf(),
                        rel_path: relative_path.to_path_buf(),
                    });
                }
                Some(HANDLEBARS_EXTENSION) => {
                    // handlebars requires template name to be unicode
                    // (we use file name as the template name).
                    match entry.path().file_name() {
                        Some(oss) if oss.to_str().is_some() => {}
                        _ => {
                            return Err(Error::NonUnicodeFilename(
                                entry.path().display().to_string(),
                            ))
                        }
                    };
                    templates.push(entry.into_path())
                }
                _ => {}
            }
        }
    }
    Ok(ScanResults {
        templates,
        markdown,
    })
}

/// get rid of files we don't care about
fn file_filter(entry: &DirEntry) -> bool {
    // ignore directories, symlinks, stdin, and stdout
    match entry.file_type() {
        None => false,
        Some(ft) if ft.is_file() => true,
        // ignore stdin, symlinks
        _ => false,
    }
}

/// ensure no fatal errors processing entry
fn ok_entry<E: std::fmt::Display>(res: std::result::Result<DirEntry, E>) -> Result<DirEntry> {
    // handle walk path errors (e.g., permission errors)
    let entry = res.map_err(|e| Error::FileScan(e.to_string()))?;
    if let Some(err) = entry.error() {
        return Err(Error::FileParse(err.to_string()));
    }
    Ok(entry)
}
