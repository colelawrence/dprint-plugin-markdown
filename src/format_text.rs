use std::path::Path;

use anyhow::bail;
use anyhow::Result;
use dprint_core::configuration::resolve_new_line_kind;
use dprint_core::formatting::*;

use super::configuration::Configuration;
use super::generation::common::Html;
use super::generation::common::Node;
use super::generation::common::Ranged;
use super::generation::common::SourceFile;
use super::generation::file_has_ignore_file_directive;
use super::generation::generate;
use super::generation::parse_cmark_ast;
use super::generation::strip_metadata_header;
use super::generation::Context;

/// Formats a file.
///
/// Returns the file text or an error when it failed to parse.
pub fn format_text(
  file_text: &str,
  config: &Configuration,
  format_code_block_text: impl for<'a> FnMut(&str, &'a str, u32) -> Result<Option<String>>,
) -> Result<Option<String>> {
  format_text_with_mode(file_text, config, FormatMode::Markdown, format_code_block_text)
}

/// Formats a file using a mode inferred from the file path.
///
/// This is intentionally separate from [`format_text`] so `.mdx` can route through an MDX-aware
/// preserve-first mode instead of being treated as an ordinary Markdown extension alias.
pub fn format_text_for_file_path(
  file_path: &Path,
  file_text: &str,
  config: &Configuration,
  format_code_block_text: impl for<'a> FnMut(&str, &'a str, u32) -> Result<Option<String>>,
) -> Result<Option<String>> {
  let mode = FormatMode::from_path(file_path);
  format_text_with_mode(file_text, config, mode, format_code_block_text)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FormatMode {
  Markdown,
  Mdx,
}

impl FormatMode {
  fn from_path(file_path: &Path) -> Self {
    match file_path.extension().and_then(|extension| extension.to_str()) {
      Some(extension) if extension.eq_ignore_ascii_case("mdx") => Self::Mdx,
      _ => Self::Markdown,
    }
  }
}

fn format_text_with_mode(
  file_text: &str,
  config: &Configuration,
  mode: FormatMode,
  format_code_block_text: impl for<'a> FnMut(&str, &'a str, u32) -> Result<Option<String>>,
) -> Result<Option<String>> {
  let result = format_text_inner(file_text, config, mode, format_code_block_text)?;

  match result {
    Some(result) if result == file_text => Ok(None),
    Some(result) => Ok(Some(result)),
    None => Ok(None),
  }
}

fn format_text_inner(
  file_text: &str,
  config: &Configuration,
  mode: FormatMode,
  mut format_code_block_text: impl for<'a> FnMut(&str, &'a str, u32) -> Result<Option<String>>,
) -> Result<Option<String>> {
  let original_file_text = strip_bom(file_text);
  let mdx_formatted_text = match mode {
    FormatMode::Markdown => None,
    FormatMode::Mdx => format_mdx_embedded_blocks(original_file_text, config.line_width, &mut format_code_block_text),
  };
  let file_text = mdx_formatted_text.as_deref().unwrap_or(original_file_text);
  let (source_file, markdown_text) = match parse_source_file(file_text, config, mode) {
    Ok(ParseFileResult::IgnoreFile) => return Ok(None),
    Ok(ParseFileResult::SourceFile(file)) => file,
    Err(_error) if mdx_formatted_text.is_some() => match parse_source_file(original_file_text, config, mode)? {
      ParseFileResult::IgnoreFile => return Ok(None),
      ParseFileResult::SourceFile(file) => file,
    },
    Err(error) => return Err(error),
  };

  Ok(Some(dprint_core::formatting::format(
    || {
      let mut context = Context::new(markdown_text, config, format_code_block_text);
      #[allow(clippy::let_and_return)]
      let print_items = generate(&source_file.into(), &mut context);
      // eprintln!("{}", print_items.get_as_text());
      print_items
    },
    config_to_print_options(original_file_text, config),
  )))
}

#[cfg(feature = "tracing")]
pub fn trace_file(
  file_text: &str,
  config: &Configuration,
  format_code_block_text: impl for<'a> FnMut(&str, &'a str, u32) -> Result<Option<String>>,
) -> dprint_core::formatting::TracingResult {
  let (source_file, markdown_text) = match parse_source_file(file_text, config, FormatMode::Markdown).unwrap() {
    ParseFileResult::IgnoreFile => panic!("Cannot trace file because it has an ignore file comment."),
    ParseFileResult::SourceFile(file) => file,
  };
  dprint_core::formatting::trace_printing(
    || {
      let mut context = Context::new(markdown_text, config, format_code_block_text);
      let print_items = generate(&source_file.into(), &mut context);
      // eprintln!("{}", print_items.get_as_text());
      print_items
    },
    config_to_print_options(file_text, config),
  )
}

fn strip_bom(text: &str) -> &str {
  text.strip_prefix("\u{FEFF}").unwrap_or(text)
}

enum ParseFileResult<'a> {
  IgnoreFile,
  SourceFile((crate::generation::common::SourceFile, &'a str)),
}

fn parse_source_file<'a>(file_text: &'a str, config: &Configuration, mode: FormatMode) -> Result<ParseFileResult<'a>> {
  // check for the presence of a dprint-ignore-file comment before parsing
  if file_has_ignore_file_directive(strip_metadata_header(file_text), &config.ignore_file_directive) {
    return Ok(ParseFileResult::IgnoreFile);
  }

  match parse_cmark_ast(file_text) {
    Ok(source_file) => {
      let source_file = match mode {
        FormatMode::Markdown => source_file,
        FormatMode::Mdx => preserve_mdx_embedded_blocks(source_file, file_text),
      };
      Ok(ParseFileResult::SourceFile((source_file, file_text)))
    }
    Err(error) => bail!(
      "{}",
      dprint_core::formatting::utils::string_utils::format_diagnostic(
        Some((error.range.start, error.range.end)),
        &error.message,
        file_text
      )
    ),
  }
}

fn format_mdx_embedded_blocks(
  file_text: &str,
  line_width: u32,
  format_embedded_text: &mut impl for<'a> FnMut(&str, &'a str, u32) -> Result<Option<String>>,
) -> Option<String> {
  let source_file = parse_cmark_ast(file_text).ok()?;
  let preserve_ranges = mdx_line_boundary_preserve_ranges(file_text);
  let mut replacements = Vec::new();

  for node in &source_file.children {
    let Some(embedded_block) = mdx_embedded_block(node, file_text) else {
      continue;
    };
    if preserve_ranges
      .iter()
      .any(|range| ranges_overlap(range, &embedded_block.range))
    {
      continue;
    }
    let block_text = &file_text[embedded_block.range.clone()];
    if let Ok(Some(formatted_text)) = format_embedded_text("tsx", block_text, line_width) {
      let formatted_text = formatted_text.trim_end().to_string();
      if embedded_block.is_safe_formatted_text(&formatted_text) && formatted_text != block_text {
        replacements.push((embedded_block.range.start, embedded_block.range.end, formatted_text));
      }
    }
  }

  if replacements.is_empty() {
    return None;
  }

  let mut result = String::with_capacity(file_text.len());
  let mut last_index = 0;
  for (start, end, replacement) in replacements {
    result.push_str(&file_text[last_index..start]);
    result.push_str(&replacement);
    last_index = end;
  }
  result.push_str(&file_text[last_index..]);
  Some(result)
}

struct MdxEmbeddedBlock {
  kind: MdxEmbeddedBlockKind,
  range: std::ops::Range<usize>,
}

impl MdxEmbeddedBlock {
  fn is_safe_formatted_text(&self, formatted_text: &str) -> bool {
    let preserves_embedded_kind = || {
      let Ok(source_file) = parse_cmark_ast(formatted_text) else {
        return false;
      };
      let mut has_embedded_block = false;
      for node in source_file.children {
        let Some(block) = mdx_embedded_block(&node, formatted_text) else {
          return false;
        };
        if block.kind != self.kind {
          return false;
        }
        has_embedded_block = true;
      }
      has_embedded_block
    };

    match self.kind {
      MdxEmbeddedBlockKind::Esm => starts_with_mdx_esm_keyword(formatted_text) && preserves_embedded_kind(),
      MdxEmbeddedBlockKind::Jsx => is_mdx_jsx_block(formatted_text) && preserves_embedded_kind(),
    }
  }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum MdxEmbeddedBlockKind {
  Esm,
  Jsx,
}

fn mdx_embedded_block(node: &Node, file_text: &str) -> Option<MdxEmbeddedBlock> {
  if let Some(range) = mdx_esm_node_range(node, file_text) {
    return Some(MdxEmbeddedBlock {
      kind: MdxEmbeddedBlockKind::Esm,
      range,
    });
  }

  if let Some(range) = mdx_jsx_node_range(node, file_text) {
    return Some(MdxEmbeddedBlock {
      kind: MdxEmbeddedBlockKind::Jsx,
      range,
    });
  }

  None
}

fn preserve_mdx_embedded_blocks(mut source_file: SourceFile, file_text: &str) -> SourceFile {
  let preserve_ranges = mdx_line_boundary_preserve_ranges(file_text);
  let mut preserve_range_index = 0;
  let mut children = Vec::with_capacity(source_file.children.len());
  let mut remaining = source_file.children.into_iter().peekable();

  while let Some(node) = remaining.next() {
    while preserve_ranges
      .get(preserve_range_index)
      .is_some_and(|range| range.end <= node.range().start)
    {
      preserve_range_index += 1;
    }
    if let Some(preserve_range) = preserve_ranges.get(preserve_range_index) {
      if range_contains_node_start(preserve_range, &node) {
        while remaining
          .peek()
          .is_some_and(|next_node| next_node.range().start < preserve_range.end)
        {
          remaining.next();
        }
        children.push(
          Html {
            range: preserve_range.clone(),
          }
          .into(),
        );
        continue;
      }
    }

    let Some(block) = mdx_embedded_block(&node, file_text) else {
      if let Some(mut raw_range) = mdx_preserve_only_node_range(&node, file_text) {
        while let Some(next_node) = remaining.peek() {
          let Some(next_range) = mdx_preserve_only_node_range(next_node, file_text) else {
            break;
          };
          if !file_text[raw_range.end..next_range.start].trim().is_empty() {
            break;
          }
          raw_range.end = next_range.end;
          remaining.next();
        }
        children.push(Html { range: raw_range }.into());
      } else {
        children.push(node);
      }
      continue;
    };
    let mut raw_range = block.range;

    while let Some(next_node) = remaining.peek() {
      let Some(next_block) = mdx_embedded_block(next_node, file_text) else {
        break;
      };
      if next_block.kind != block.kind || !file_text[raw_range.end..next_block.range.start].trim().is_empty() {
        break;
      }
      raw_range.end = next_block.range.end;
      remaining.next();
    }

    children.push(Html { range: raw_range }.into());
  }

  source_file.children = children;
  source_file
}

fn mdx_line_boundary_preserve_ranges(file_text: &str) -> Vec<std::ops::Range<usize>> {
  let lines = collect_line_ranges(file_text);
  let mut preserve_ranges = Vec::new();
  let mut fenced_code_kind = None;

  for (index, line) in lines.iter().enumerate() {
    let text = &file_text[line.clone()];
    let trimmed = text.trim_start();
    if let Some(kind) = fenced_code_delimiter_kind(trimmed) {
      if fenced_code_kind == Some(kind) {
        fenced_code_kind = None;
      } else if fenced_code_kind.is_none() {
        fenced_code_kind = Some(kind);
      }
      continue;
    }
    if fenced_code_kind.is_some() || has_leading_indent(text) || !is_mdx_jsx_opening_line(trimmed) {
      continue;
    }
    let Some(tag_name) = mdx_jsx_tag_name(trimmed) else {
      continue;
    };
    if single_line_jsx_contains_markdown_child(trimmed, tag_name) {
      preserve_ranges.push(line.clone());
      continue;
    }
    let Some(end_index) = find_top_level_jsx_closing_line(&lines, file_text, index + 1, tag_name) else {
      continue;
    };
    if jsx_range_contains_markdown_child(&lines, file_text, index + 1, end_index) {
      preserve_ranges.push(line.start..lines[end_index].end);
    }
  }

  preserve_ranges
}

fn collect_line_ranges(file_text: &str) -> Vec<std::ops::Range<usize>> {
  let mut ranges = Vec::new();
  let mut start = 0;
  for line in file_text.split_inclusive('\n') {
    let end = start + line.trim_end_matches(['\r', '\n']).len();
    ranges.push(start..end);
    start += line.len();
  }
  if start < file_text.len() {
    ranges.push(start..file_text.len());
  }
  ranges
}

fn single_line_jsx_contains_markdown_child(text: &str, tag_name: &str) -> bool {
  let closing_start = format!("</{}", tag_name);
  let Some(open_end) = text.find('>') else {
    return false;
  };
  let Some(close_start) = text.rfind(&closing_start) else {
    return false;
  };
  close_start > open_end && is_markdown_child_line(&text[open_end + 1..close_start])
}

fn find_top_level_jsx_closing_line(
  lines: &[std::ops::Range<usize>],
  file_text: &str,
  start_index: usize,
  tag_name: &str,
) -> Option<usize> {
  let mut nested_same_tag_depth = 0;
  let mut fenced_code_kind = None;
  for (index, line) in lines.iter().enumerate().skip(start_index) {
    let text = file_text[line.clone()].trim_start();
    if let Some(kind) = fenced_code_delimiter_kind(text) {
      if fenced_code_kind == Some(kind) {
        fenced_code_kind = None;
      } else if fenced_code_kind.is_none() {
        fenced_code_kind = Some(kind);
      }
      continue;
    }
    if fenced_code_kind.is_some() {
      continue;
    }
    if is_mdx_jsx_opening_line(text)
      && mdx_jsx_tag_name(text) == Some(tag_name)
      && !single_line_jsx_has_closing_tag(text, tag_name)
    {
      nested_same_tag_depth += 1;
      continue;
    }
    if is_mdx_jsx_closing_line(text, tag_name) {
      if nested_same_tag_depth == 0 {
        return Some(index);
      }
      nested_same_tag_depth -= 1;
    }
  }
  None
}

fn jsx_range_contains_markdown_child(
  lines: &[std::ops::Range<usize>],
  file_text: &str,
  start_index: usize,
  end_index: usize,
) -> bool {
  lines[start_index..end_index].iter().any(|line| {
    let text = file_text[line.clone()].trim_start();
    is_markdown_child_line(text)
  })
}

fn is_markdown_child_line(text: &str) -> bool {
  let text = text.trim_start();
  !text.is_empty() && !text.starts_with('<') && !text.starts_with('{')
}

fn is_mdx_jsx_opening_line(text: &str) -> bool {
  is_mdx_jsx_block(text) && !text.starts_with("</") && !text.ends_with("/>")
}

fn is_mdx_jsx_closing_line(text: &str, tag_name: &str) -> bool {
  let Some(rest) = text.strip_prefix("</") else {
    return false;
  };
  let Some(rest) = rest.strip_prefix(tag_name) else {
    return false;
  };
  rest.starts_with('>') || rest.chars().next().is_some_and(char::is_whitespace)
}

fn single_line_jsx_has_closing_tag(text: &str, tag_name: &str) -> bool {
  let closing_start = format!("</{}", tag_name);
  text.find('>').is_some_and(|open_end| {
    text[open_end + 1..]
      .find(&closing_start)
      .is_some_and(|close_start| is_mdx_jsx_closing_line(&text[open_end + 1 + close_start..], tag_name))
  })
}

fn mdx_jsx_tag_name(text: &str) -> Option<&str> {
  let rest = text.strip_prefix('<')?;
  if rest.starts_with('>') || rest.starts_with('/') {
    return None;
  }
  let tag_name_end = rest
    .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
    .unwrap_or(rest.len());
  if tag_name_end == 0 {
    None
  } else {
    Some(&rest[..tag_name_end])
  }
}

fn fenced_code_delimiter_kind(text: &str) -> Option<char> {
  if text.starts_with("```") {
    Some('`')
  } else if text.starts_with("~~~") {
    Some('~')
  } else {
    None
  }
}

fn has_leading_indent(text: &str) -> bool {
  text.starts_with(' ') || text.starts_with('\t')
}

fn ranges_overlap(left: &std::ops::Range<usize>, right: &std::ops::Range<usize>) -> bool {
  left.start < right.end && right.start < left.end
}

fn range_contains_node_start(range: &std::ops::Range<usize>, node: &Node) -> bool {
  let node_start = node.range().start;
  range.start <= node_start && node_start < range.end
}

fn mdx_preserve_only_node_range(node: &Node, file_text: &str) -> Option<std::ops::Range<usize>> {
  match node {
    Node::Paragraph(paragraph)
      if is_standalone_mdx_expression_block(&file_text[paragraph.range.clone()])
        || is_mdx_import_like_non_esm_block(&file_text[paragraph.range.clone()]) =>
    {
      Some(paragraph.range.clone())
    }
    _ => None,
  }
}

fn mdx_esm_node_range(node: &Node, file_text: &str) -> Option<std::ops::Range<usize>> {
  match node {
    Node::Paragraph(paragraph) if is_mdx_esm_block(&file_text[paragraph.range.clone()]) => {
      Some(paragraph.range.clone())
    }
    _ => None,
  }
}

fn mdx_jsx_node_range(node: &Node, file_text: &str) -> Option<std::ops::Range<usize>> {
  match node {
    Node::Html(html) if is_mdx_jsx_block(&file_text[html.range.clone()]) => Some(html.range.clone()),
    Node::Paragraph(paragraph) if is_mdx_jsx_paragraph(&file_text[paragraph.range.clone()]) => {
      Some(paragraph.range.clone())
    }
    _ => None,
  }
}

fn is_mdx_esm_block(text: &str) -> bool {
  starts_with_mdx_esm_keyword(text)
}

fn starts_with_mdx_esm_keyword(text: &str) -> bool {
  let first_line = text.lines().find(|line| !line.trim().is_empty()).unwrap_or("");
  let first_line = first_line.trim_start();

  is_mdx_import_declaration_start(first_line) || starts_with_keyword_and_whitespace(first_line, "export")
}

fn is_mdx_jsx_block(text: &str) -> bool {
  let text = text.trim_start();
  text.starts_with('<')
    && text.trim_end().ends_with('>')
    && !text.starts_with("<!--")
    && !text.starts_with("<!")
    && !text.starts_with("<?")
}

fn is_mdx_jsx_paragraph(text: &str) -> bool {
  let text = text.trim();
  is_mdx_jsx_block(text) && text.ends_with('>')
}

fn is_standalone_mdx_expression_block(text: &str) -> bool {
  let text = text.trim();
  text.starts_with('{') && text.ends_with('}')
}

fn is_mdx_import_like_non_esm_block(text: &str) -> bool {
  let first_line = text.lines().find(|line| !line.trim().is_empty()).unwrap_or("");
  let first_line = first_line.trim_start();
  starts_with_keyword_and_whitespace(first_line, "import") && !is_mdx_import_declaration_start(first_line)
}

fn is_mdx_import_declaration_start(text: &str) -> bool {
  let Some(rest) = text.strip_prefix("import") else {
    return false;
  };
  let Some(first_char) = rest.chars().next() else {
    return false;
  };
  if !first_char.is_whitespace() {
    return false;
  }
  let rest = rest.trim_start();
  !rest.starts_with('(') && !rest.starts_with('.')
}

fn starts_with_keyword_and_whitespace(text: &str, keyword: &str) -> bool {
  let Some(rest) = text.strip_prefix(keyword) else {
    return false;
  };
  rest.chars().next().is_some_and(char::is_whitespace)
}

fn config_to_print_options(file_text: &str, config: &Configuration) -> PrintOptions {
  PrintOptions {
    indent_width: 1, // force
    max_width: config.line_width,
    use_tabs: false, // ignore tabs, always use spaces
    new_line_text: resolve_new_line_kind(file_text, config.new_line_kind),
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use crate::configuration::ConfigurationBuilder;

  #[test]
  fn strips_bom() {
    for input_text in ["\u{FEFF}#  Title", "\u{FEFF}# Title\n"] {
      let config = ConfigurationBuilder::new().build();
      let result = format_text(input_text, &config, |_, _, _| Ok(None)).unwrap();
      assert_eq!(result, Some("# Title\n".to_string()));
    }
  }

  #[test]
  fn mdx_embedded_formatting_preserves_original_newline_kind() {
    let config = ConfigurationBuilder::new()
      .new_line_kind(dprint_core::configuration::NewLineKind::Auto)
      .build();
    let input_text = "import {A,B} from \"x\"\r\n\r\n#  Hello\r\n";
    let result = format_text_for_file_path(Path::new("file.mdx"), input_text, &config, |tag, file_text, _| {
      if tag == "tsx" {
        Ok(Some(file_text.replace("{A,B}", "{ A, B }")))
      } else {
        Ok(None)
      }
    })
    .unwrap();
    assert_eq!(
      result,
      Some("import { A, B } from \"x\"\r\n\r\n# Hello\r\n".to_string())
    );
  }
}
