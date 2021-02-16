
v0.2.1

- add visual diff for markdown. Uses dissimilar crate to generate diffs,
  and inserts <span> tags before and after each insertion or deletion.
  The inserted are configurable (don't even need to be span).

- renderer.set() is more flexible in its value parameter: takes Into<TomlValue> instead of TomlValue.

- render.write_page_html() replaces the crate function write_page_html
