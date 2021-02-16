//! HTML generation
//!
use crate::{Result, TomlMap};
use chrono::DateTime;
use handlebars::Handlebars;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use toml::value::Value as TomlValue;

/// Html to insert before and after diff chunks
pub struct DiffStyle {
    /// Html to insert before a span of inserted content
    /// `<span class="...">`
    pub ins_start: String,
    /// Html to insert after a span of inserted content
    /// `</span>`
    pub ins_end: String,
    /// Html to insert before a span of deleted content
    /// `<span class="...">`
    pub del_start: String,
    /// Html to insert after a span of deleted content
    /// `</span>`
    pub del_end: String,
}

impl Default for DiffStyle {
    fn default() -> DiffStyle {
        DiffStyle {
            ins_start: r#"<span class="bg-green-100 text-gray-600">"#.to_string(),
            ins_end: r#"</span>"#.to_string(),
            del_start: r#"<span class="bg-red-100 text-gray-600 line-through">"#.to_string(),
            del_end: r#"</span>"#.to_string(),
        }
    }
}

// these defaults can be overridden by the config file
/// Pairing of template name and contents
///
pub type Template<'template> = (&'template str, &'template str);

#[derive(Debug)]
pub struct RenderConfig<'render> {
    /// Templates to be loaded for renderer. List of template name, data
    pub templates: Vec<Template<'render>>,
    /// Whether parser is in strict mode (e.g. if true, a variable used in template
    /// that is undefined would raise an error; if false, it would evaluate to 'falsey'
    pub strict_mode: bool,
}

impl<'render> Default for RenderConfig<'render> {
    fn default() -> Self {
        Self {
            templates: Vec::new(),
            strict_mode: false,
        }
    }
}

/// HBTemplate processor for HTML generation
pub struct Renderer<'gen> {
    /// Handlebars processor
    hb: Handlebars<'gen>,
    /// Additional dictionary that supplements data passed to render() method
    vars: TomlMap,
}

impl<'gen> Default for Renderer<'gen> {
    fn default() -> Self {
        // unwrap ok because only error condition occurs with templates, and default has none.
        Self::init(&RenderConfig::default()).unwrap()
    }
}

impl<'gen> Renderer<'gen> {
    /// Initialize handlebars template processor.
    pub fn init(config: &RenderConfig) -> Result<Self> {
        let mut hb = Handlebars::new();
        // don't use strict mode because docs may have different frontmatter vars
        // and it's easier in templates to use if we allow undefined ~= false-y
        hb.set_strict_mode(config.strict_mode);
        hb.register_escape_fn(handlebars::no_escape); //html escaping is the default and cause issue0
        add_base_helpers(&mut hb);

        for t in &config.templates {
            hb.register_template_string(t.0, t.1)?;
        }

        let renderer = Self {
            hb,
            vars: TomlMap::new(),
        };
        Ok(renderer)
    }

    /// Replace renderer dict.
    /// Values in the renderer dict override any values passed to render()
    pub fn set_vars(&mut self, vars: TomlMap) {
        self.vars = vars
    }

    /// Sets all the vars from the hashap into the render dict
    pub fn set_from<T: Into<toml::Value>>(&mut self, vars: HashMap<String, T>) {
        for (k, v) in vars.into_iter() {
            self.set(k, v);
        }
    }

    /// Set a value in the renderer dict. If the key was previously set, it is replaced.
    /// Values in the renderer dict override any values passed to render()
    pub fn set<T: Into<TomlValue>>(&mut self, key: String, val: T) {
        self.vars.insert(key, val.into());
    }

    /// Remove key if it was present
    pub fn remove(&mut self, key: &str) {
        self.vars.remove(key);
    }

    /// Adds template to internal dictionary
    pub fn add_template(&mut self, template: Template) -> Result<()> {
        self.hb.register_template_string(template.0, template.1)?;
        Ok(())
    }

    /// Render a template with data.
    pub fn render<W>(&self, template_name: &str, mut data: TomlMap, writer: &mut W) -> Result<()>
    where
        W: std::io::Write,
    {
        // add variables that extend/override passed data
        data.extend(self.vars.clone().into_iter());
        self.hb.render_to_write(template_name, &data, writer)?;
        Ok(())
    }

    /// Convert markdown to html and generate html page,
    /// using 'map' data as render vars
    pub fn write_page_html<W: std::io::Write>(
        &self,
        mut map: TomlMap,
        markdown: &str,
        template_name: &str,
        mut writer: &mut W,
    ) -> Result<()> {
        let html = crate::md_parser::markdown_to_html(markdown)?;
        map.insert("content".into(), TomlValue::from(html.content));
        if let Some(toc) = html.toc {
            map.insert("toc".into(), TomlValue::from(toc));
        }
        self.render(template_name, map, &mut writer)?;
        Ok(())
    }
}

/// Convert Value to string without adding quotes around strings
fn json_value_to_string(v: &JsonValue) -> String {
    match v {
        JsonValue::String(s) => s.clone(),
        _ => v.to_string(),
    }
}

/// Add template helpers functions
///  'join-csv' turns array of values into comma-separate list
///  'format-date' rewrites an ISO8601-formatted date into another format
fn add_base_helpers(hb: &mut Handlebars) {
    use handlebars::{Context, Helper, HelperResult, Output, RenderContext, RenderError};

    // "join-csv" turns array of values into comma-separated list
    // Converts each value using to_string()
    hb.register_helper(
        "join-csv",
        Box::new(
            |h: &Helper,
             _r: &Handlebars,
             _: &Context,
             _rc: &mut RenderContext,
             out: &mut dyn Output|
             -> HelperResult {
                let csv = h
                    .param(0)
                    .ok_or_else(|| RenderError::new("param not found"))?
                    .value()
                    .as_array()
                    .ok_or_else(|| RenderError::new("expected array"))?
                    .iter()
                    .map(json_value_to_string)
                    .collect::<Vec<String>>()
                    .join(",");
                out.write(&csv)?;
                Ok(())
            },
        ),
    );
    //
    // format-date: strftime-like function to reformat date
    hb.register_helper(
        "format-date",
        Box::new(
            |h: &Helper,
             _r: &Handlebars,
             _: &Context,
             _rc: &mut RenderContext,
             out: &mut dyn Output|
             -> HelperResult {
                // get first arg as string, an ISO8601-formatted date
                let date = h
                    .param(0)
                    .ok_or_else(|| RenderError::new("expect first param as date"))?
                    .value()
                    .as_str()
                    .ok_or_else(|| RenderError::new("expect strings"))?;
                // parse into DateTime
                let date = DateTime::parse_from_rfc3339(date)
                    .map_err(|e| RenderError::from_error("date parse", e))?;
                // get second arg - the format string
                let format = h
                    .param(1)
                    .ok_or_else(|| RenderError::new("expect second param as format"))?
                    .value()
                    .as_str()
                    .ok_or_else(|| RenderError::new("expect strings"))?;
                // print date in specified format
                let formatted = date.format(format).to_string();
                out.write(&formatted)?;
                Ok(())
            },
        ),
    );
}

/// Generate diff between two text segments.
/// Enclose additions with <span class="add_style">...</span>
/// and deletions with <span class="del_style">
/// add_style, e.g., "bg-green 100 text-gray-500"
///
pub fn generate_diff(first: &str, second: &str, style: &DiffStyle) -> Result<String> {
    use dissimilar::Chunk;

    let chunks = dissimilar::diff(&first, &second);

    // "<span class=\"bg-red-100 text-gray-600 line-through\">");
    // <span class=\"bg-green-100 text-gray-600\">");
    let mut diff_content = String::with_capacity(second.len() + 1048 + 30 * chunks.len());
    for chunk in chunks.iter() {
        match chunk {
            Chunk::Equal(s) => {
                diff_content.push_str(s);
            }
            Chunk::Delete(s) => {
                diff_content.push_str(&style.del_start);
                diff_content.push_str(s);
                diff_content.push_str(&style.del_end);
            }
            Chunk::Insert(s) => {
                diff_content.push_str(&style.ins_start);
                diff_content.push_str(s);
                diff_content.push_str(&style.ins_end);
            }
        }
    }
    Ok(diff_content)
}

#[test]
fn initializers() {
    let mut r1 = Renderer::default();
    r1.set("x".into(), toml::Value::from("xyz"));
    assert!(true);

    let mut r2 = Renderer::init(&RenderConfig::default()).expect("ok");
    r2.set("x".into(), toml::Value::from("xyz"));
    assert!(true);
}

/// Test template processor
#[test]
fn test_html_page() {
    use crate::render::Renderer;
    const TEST_TEMPLATE: &str = "<html><body><h1>{{title}}</h1>{{content}}</body></html>";

    let mut map = TomlMap::new();
    map.insert("title".into(), "Abc".into());

    // simulate processing
    let expected = TEST_TEMPLATE
        .replace("{{content}}", "<p>hello</p>")
        .replace("{{title}}", "Abc");

    let mut map = TomlMap::new();
    map.insert("title".into(), "Abc".into());

    let mut gen = Renderer::default();
    gen.add_template(("test_template", TEST_TEMPLATE))
        .expect("add test template");

    let mut buf: Vec<u8> = Vec::new();
    let result = gen.write_page_html(map, "hello", "test_template", &mut buf);
    assert!(result.is_ok());

    // had to remove newlines - there's an added \n after
    let output = String::from_utf8_lossy(&buf).replace("\n", "");
    assert_eq!(expected, output);
}
