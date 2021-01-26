//! Markdown parser - parses markdown and generates html
//! Also generates TOC if the markdown contains a toc-generation flag
//!
use crate::Result;
use pulldown_cmark::{Event, Options as MdOptions, Parser, Tag};

/// Max depth of generated TOC: 3 is usually enough, 4 is bordering on excessive
const MAX_TOC_DEPTH: u8 = 4;
/// Flag in markdown to generate TOC
const TOC_FLAG: &str = "<!-- toc -->";

// use div and p instead of ul and li - better typography
// stylesheet adds a left margin to each div to make it indented
const TOC_INDENT: &str = "<div>";
const TOC_END_INDENT: &str = "</div>";
const TOC_ITEM: &str = "<p>";
const TOC_END_ITEM: &str = "</p>";

/// html result from markdown parser
#[derive(Debug)]
pub struct ParseResult {
    /// markdown content converted to html
    pub content: String,
    /// table of contents, if toc flag was found in source
    pub toc: Option<String>,
}

/// State machine for parsing markdown headings (h1, h2, ...)
/// Idle (Not in heading)
/// -> HeadingStarted (heading start event index, heading level)
/// -> HeadingTextParsed ((head-start-index,text-index), level, text)
/// -> HeadingComplete -> return to Idle
#[derive(Debug)]
enum HeadingParseState {
    // Not in heading
    Idle,
    // heading start. params= (heading start event index, heading level)
    HeadingStarted(usize, u8),
    // text parsed (heading "mid") params= ((head-start-index,text-index), level, text)
    HeadingTextParsed((usize, usize), u8, String),
    // heading end; return to Idle
}

/// Result of parsing document headings (h1, h2, ...)
#[derive(Debug)]
struct Heading {
    // indices for event objects: heading-start, text, heading-end
    index: (usize, usize, usize),
    // heading level (1-n)
    level: u8,
    // heading text
    text: String,
    // anchor slug
    slug: String,
}

impl Heading {
    /// Generate html start tag, "<h_ id="slug">"
    fn html_start_element(&self) -> String {
        format!(
            "<h{level} id=\"{slug}\">",
            level = self.level,
            slug = &self.slug,
        )
    }
}

/// Turn heading into anchor slug, e.g. "Where am I?" -> "where-am-i"
fn slugify_heading_for_anchor(s: &str) -> String {
    slug::slugify(s)
}

/// Gather headings for inserting into toc, and give heading nodes an id
/// Using a mini-state machine to track start of heading, heading text, end of heading
fn fix_headings(events: &mut [Event]) -> Vec<Heading> {
    use HeadingParseState::{HeadingStarted, HeadingTextParsed, Idle};
    let mut state: HeadingParseState = Idle;
    let mut headings = Vec::new();

    for (i, event) in events.iter().enumerate() {
        match (event, &state) {
            // was idle, found heading start
            (Event::Start(Tag::Heading(level)), Idle) => {
                state = HeadingStarted(i, *level as u8);
            }
            // have seen start and text
            (Event::Text(text), HeadingStarted(ix, level)) => {
                state = HeadingTextParsed((*ix, i), *level, text.to_string());
            }
            // have start, text, and end: heading complete. Save, and reset to idle
            (
                Event::End(Tag::Heading(end_level)),
                HeadingTextParsed((start_ix, text_ix), start_level, text),
            ) if *end_level as u8 == *start_level => {
                headings.push(Heading {
                    index: (*start_ix, *text_ix, i),
                    level: *start_level,
                    text: text.clone(),
                    slug: slugify_heading_for_anchor(text),
                });
                state = Idle;
            }
            _ => {}
        }
    }
    // Replace all start heading element Events to write <h_ id="slug"> instead of <h_>
    for h in headings.iter() {
        let (start_ix, _text_ix, _end_ix) = h.index;
        events[start_ix] = Event::Html(h.html_start_element().into());
    }
    headings
}

/// Parse content markdown and generate html, with optional generation of TOC
/// Markdown parameter should not have frontmatter
pub fn markdown_to_html(markdown_in: &str) -> Result<ParseResult> {
    use pulldown_cmark::CowStr;
    let mut enable_toc = false;

    let mut options = MdOptions::empty();
    // enable the following extensions: strikethrough, git tables, task lists
    options.insert(MdOptions::ENABLE_STRIKETHROUGH);
    options.insert(MdOptions::ENABLE_TABLES);
    options.insert(MdOptions::ENABLE_TASKLISTS);

    // Parse markdown into array of events, so we can do multiple passes
    let mut events = Parser::new_ext(markdown_in, options)
        .enumerate()
        .map(|(_i, event)| match event {
            // Do some simple link checking/fixing
            Event::Start(Tag::Link(link_type, dest, title)) if dest.is_empty() => {
                Event::Start(Tag::Link(link_type, "#".into(), title))
            }
            Event::Html(markup) => {
                if markup.contains(TOC_FLAG) {
                    enable_toc = true;
                    Event::Html(CowStr::from(markup.replacen(TOC_FLAG, "", 1)))
                } else {
                    Event::Html(markup)
                }
            }
            _ => event,
        })
        .collect::<Vec<_>>(); // collect events for additional passes;

    // If there was a flag requesting toc, generate toc and add anchor tags to headings
    let toc = if enable_toc {
        let headings = fix_headings(&mut events);
        Some(generate_toc_html(&headings, MAX_TOC_DEPTH))
    } else {
        None
    };

    let mut content = String::with_capacity(markdown_in.len());
    pulldown_cmark::html::push_html(&mut content, events.into_iter());
    Ok(ParseResult { content, toc })
}

/// Generate TOC item: html link inside a list item tag
fn toc_item_html(href: &str, text: &str) -> String {
    format!(
        "{begin}<a href=\"#{href}\">{text}</a>{end}",
        begin = TOC_ITEM,
        href = href,
        text = text,
        end = TOC_END_ITEM,
    )
}

/// Use headings array to generate TOC in HTML
fn generate_toc_html(headings: &[Heading], max_depth: u8) -> String {
    use std::cmp::Ordering;

    let mut html = String::with_capacity(headings.len() * 15);
    let mut indent: u8 = 0;
    for h in headings
        .iter()
        .filter(|h| h.level >= 1 && h.level <= max_depth)
    {
        match h.level.cmp(&indent) {
            Ordering::Greater => {
                html.push_str(&TOC_INDENT.repeat((h.level - indent) as usize));
                indent = h.level;
            }
            Ordering::Less => {
                html.push_str(&TOC_END_INDENT.repeat((indent - h.level) as usize));
                indent = h.level;
            }
            Ordering::Equal => {}
        }
        html.push_str(&toc_item_html(&h.slug, &h.text));
    }
    html.push_str(&TOC_END_INDENT.repeat(indent as usize));
    html
}

#[test]
fn test_slugify() {
    assert_eq!(slugify_heading_for_anchor("a b c"), "a-b-c", "spaces");
    assert_eq!(
        slugify_heading_for_anchor("  a  "),
        "a",
        "remove leading and trailing spaces"
    );
    assert_eq!(
        slugify_heading_for_anchor("-a-b-"),
        "a-b",
        "remove leading and trailing dash"
    );
    assert_eq!(
        slugify_heading_for_anchor("\ta*/+()b"),
        "a-b",
        "multiple dashes coalesce into one"
    );
    assert_eq!(
        slugify_heading_for_anchor("a__b"),
        "a-b",
        "replace underscore"
    );
    assert_eq!(slugify_heading_for_anchor("a.b"), "a-b", "replace period");
    assert_eq!(slugify_heading_for_anchor("a-b"), "a-b", "dash ok");
    assert_eq!(slugify_heading_for_anchor("α-ω"), "a-o", "no non-ascii");
}
