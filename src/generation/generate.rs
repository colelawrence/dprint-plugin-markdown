use dprint_core::formatting::condition_resolvers;
use dprint_core::formatting::conditions::*;
use dprint_core::formatting::ir_helpers::*;
use dprint_core::formatting::*;
use dprint_core_macros::sc;
use pulldown_cmark::MetadataBlockKind;
use std::borrow::Cow;
use std::rc::Rc;
use unicode_width::UnicodeWidthStr;

use super::common::*;
use super::gen_types::*;
use super::utils;
use crate::configuration::*;

pub fn generate(node: &Node, context: &mut Context) -> PrintItems {
  // eprintln!("Kind: {:?}", node.kind());
  // eprintln!("Text: {:?}", node.text(context));

  match node {
    Node::SourceFile(node) => gen_source_file(node, context),
    Node::Heading(node) => gen_heading(node, context),
    Node::Paragraph(node) => gen_paragraph(node, context),
    Node::BlockQuote(node) => gen_block_quote(node, context),
    Node::CodeBlock(node) => gen_code_block(node, context),
    Node::Code(node) => gen_code(node, context),
    Node::Text(node) => gen_text(node, context),
    Node::TextDecoration(node) => gen_text_decoration(node, context),
    Node::Html(node) => gen_html(node, context),
    Node::DisplayMath(node) => gen_display_math(node, context),
    Node::InlineMath(node) => gen_inline_math(node, context),
    Node::FootnoteReference(node) => gen_footnote_reference(node, context),
    Node::FootnoteDefinition(node) => gen_footnote_definition(node, context),
    Node::InlineLink(node) => gen_inline_link(node, context),
    Node::ReferenceLink(node) => gen_reference_link(node, context),
    Node::ShortcutLink(node) => gen_shortcut_link(node, context),
    Node::AutoLink(node) => gen_auto_link(node, context),
    Node::LinkReference(node) => gen_link_reference(node, context),
    Node::InlineImage(node) => gen_inline_image(node, context),
    Node::ReferenceImage(node) => gen_reference_image(node, context),
    Node::List(node) => gen_list(node, false, context),
    Node::Item(node) => gen_item(node, context),
    Node::TaskListMarker(_) => unreachable!("this should be handled by gen_paragraph"),
    Node::HorizontalRule(node) => gen_horizontal_rule(node, context),
    Node::SoftBreak(_) => PrintItems::new(),
    Node::HardBreak(_) => gen_hard_break(context),
    Node::Table(node) => gen_table(node, context),
    Node::TableHead(_) => unreachable!(),
    Node::TableRow(_) => unreachable!(),
    Node::TableCell(node) => gen_table_cell(node, context),
    Node::MetadataBlock(node) => gen_metadata_block(node, context),
    Node::NotImplemented(_) => ir_helpers::gen_from_raw_string(node.text(context)),
  }
}

fn gen_source_file(source_file: &SourceFile, context: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();

  items.extend(gen_nodes(&source_file.children, context));

  items.push_condition(if_true(
    "endOfFileNewLine",
    Rc::new(|context| Some(context.writer_info.column_number > 0 || context.writer_info.line_number > 0)),
    Signal::NewLine.into(),
  ));

  items
}

fn gen_nodes(nodes: &[Node], context: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();
  if nodes.is_empty() {
    return items;
  }

  let mut last_node: Option<&Node> = None;
  let mut node_iterator = nodes.iter().filter(|n| !matches!(n, Node::SoftBreak(_)));

  while let Some(mut node) = node_iterator.next() {
    // handle alternate lists
    if let Some(Node::List(last_list)) = &last_node {
      if let Node::List(list) = &node {
        if last_list.start_index.is_some() == list.start_index.is_some() {
          items.extend(get_conditional_blank_line(node.range(), context));
          items.extend(gen_list(list, true, context));
          if let Some(current_node) = node_iterator.next() {
            last_node = Some(node);
            node = current_node;
          } else {
            break;
          }
        }
      }
    }

    // todo: this area needs to be thought out more
    if let Some(last_node) = last_node {
      if matches!(
        node,
        Node::Heading(_)
          | Node::Paragraph(_)
          | Node::CodeBlock(_)
          | Node::FootnoteDefinition(_)
          | Node::HorizontalRule(_)
          | Node::List(_)
          | Node::Table(_)
          | Node::BlockQuote(_)
      ) {
        items.extend(get_conditional_blank_line(node.range(), context));
      } else if !matches!(node, Node::HardBreak(_)) {
        match last_node {
          Node::Heading(_)
          | Node::Paragraph(_)
          | Node::CodeBlock(_)
          | Node::FootnoteDefinition(_)
          | Node::HorizontalRule(_)
          | Node::List(_)
          | Node::Table(_)
          | Node::MetadataBlock(_)
          | Node::BlockQuote(_)
          | Node::DisplayMath(_) => {
            items.extend(get_conditional_blank_line(node.range(), context));
          }
          Node::Code(_)
          | Node::SoftBreak(_)
          | Node::TextDecoration(_)
          | Node::FootnoteReference(_)
          | Node::InlineLink(_)
          | Node::ReferenceLink(_)
          | Node::ShortcutLink(_)
          | Node::AutoLink(_)
          | Node::Text(_)
          | Node::Html(_)
          | Node::InlineImage(_)
          | Node::ReferenceImage(_)
          | Node::InlineMath(_) => {
            let between_range = (last_node.range().end, node.range().start);
            let new_line_count = context.get_new_lines_in_range(between_range.0, between_range.1);

            if new_line_count == 1 {
              // Callout example:
              // > [!NOTE]
              // > Some note.
              let is_callout = if context.is_in_block_quote() {
                if let Node::Text(text) = last_node {
                  is_callout_text(&text.text)
                } else {
                  false
                }
              } else {
                false
              };
              if is_callout && !context.is_text_wrap_disabled() {
                items.push_signal(Signal::NewLine); // force a newline
              } else if matches!(node, Node::Html(_)) {
                items.push_signal(Signal::NewLine);
              } else {
                items.extend(get_newline_wrapping_based_on_config(context));
              }
            } else if new_line_count > 1 {
              items.push_signal(Signal::NewLine);
              items.push_signal(Signal::NewLine);
            } else {
              let needs_space = if let Node::Html(_) = last_node {
                node.has_preceding_space(context.file_text)
              } else if matches!(last_node, Node::Text(_)) || matches!(node, Node::Text(_)) {
                node.has_preceding_space(context.file_text)
                  || !last_node.ends_with_punctuation(context.file_text)
                    && !node.starts_with_punctuation(context.file_text)
              } else if let Node::FootnoteReference(_) = node {
                false
              } else if let Node::Html(_) = node {
                node.has_preceding_space(context.file_text)
              } else {
                true
              };

              if needs_space {
                if node.starts_with_list_word() {
                  items.push_space();
                } else {
                  items.extend(get_space_or_newline_based_on_config(context));
                }
              }
            }
          }
          Node::LinkReference(_) => {
            let needs_newline = matches!(node, Node::LinkReference(_));
            if needs_newline {
              items.push_signal(Signal::NewLine);
            }
          }
          Node::NotImplemented(_)
          | Node::SourceFile(_)
          | Node::Item(_)
          | Node::TaskListMarker(_)
          | Node::HardBreak(_)
          | Node::TableHead(_)
          | Node::TableRow(_)
          | Node::TableCell(_) => {}
        }
      }
    }

    items.extend(generate(node, context));
    last_node = Some(node);

    // check for ignore comment
    if let Node::Html(html) = node {
      let html_text = &context.file_text[html.range.clone()];
      if context.ignore_regex.is_match(html_text) {
        items.push_signal(Signal::NewLine);
        if let Some(node) = node_iterator.next() {
          if utils::has_leading_blankline(node.range().start, context.file_text) {
            items.push_signal(Signal::NewLine);
          }

          // include the leading indent
          let range = node.range();
          let text_start = utils::get_leading_non_space_tab_byte_pos(context.file_text, range.start);
          items.extend(ir_helpers::gen_from_raw_string(
            context.file_text[text_start..range.end].trim_end(),
          ));

          last_node = Some(node);
        }
      } else if context.ignore_start_regex.is_match(html_text) {
        let mut range: Option<Range> = None;
        let mut end_comment = None;
        let start = html.range().end;
        for node in node_iterator.by_ref() {
          last_node = Some(node);

          if let Node::Html(html) = node {
            let html_text = &context.file_text[html.range.clone()];
            if context.ignore_end_regex.is_match(html_text) {
              end_comment = Some(html);
              break;
            }
          }

          let node_range = node.range();
          range = Some(Range {
            start: range.map(|r| r.start).unwrap_or(node_range.start),
            end: node_range.end,
          });
        }

        let end = end_comment
          .map(|c| c.range().start)
          .unwrap_or_else(|| last_node.unwrap().range().end);
        let ignore_text = &context.file_text[start..end];
        if let Some(end_comment) = end_comment {
          items.extend(ir_helpers::gen_from_raw_string(ignore_text));
          items.extend(gen_html(end_comment, context));
        } else {
          items.extend(ir_helpers::gen_from_raw_string(ignore_text.trim_end()));
        }
      }
    }
  }

  return items;

  fn get_conditional_blank_line(range: &Range, context: &mut Context) -> PrintItems {
    let mut items = PrintItems::new();
    if !context.is_in_list() || utils::has_leading_blankline(range.start, context.file_text) {
      items.push_signal(Signal::NewLine);
    }
    items.push_signal(Signal::NewLine);
    items
  }
}

fn gen_heading(heading: &Heading, context: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();

  if heading.level < 3 && context.configuration.heading_kind == HeadingKind::Setext {
    // setext headings only apply to level 1 and level 2.
    let heading_children = gen_nodes(&heading.children, context);
    let (heading_children, cloned_children) = clone_items(heading_children);
    items.extend(heading_children);
    items.push_item(PrintItem::Signal(Signal::NewLine));

    // render the heading text with the actual line width so wrapping is
    // applied, then measure the longest line for the underline width.
    let underline_width = measure_longest_line_width(cloned_children, context.configuration.line_width);
    let underline_char = if heading.level == 1 { "=" } else { "-" };
    items.push_string(underline_char.repeat(underline_width));
  } else {
    // atx headings apply to all levels.
    items.push_string(format!("{} ", "#".repeat(heading.level as usize)));
    items.extend(with_no_new_lines(gen_nodes(&heading.children, context)));
  }

  items
}

fn gen_paragraph(paragraph: &Paragraph, context: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();

  if let Some(marker) = &paragraph.marker {
    items.extend(gen_task_list_marker(marker, context));
    if !paragraph.children.is_empty() {
      items.push_space();
    }
  }

  items.extend(gen_task_list_marker_children(
    &paragraph.children,
    paragraph.marker.as_ref(),
    context,
  ));
  items
}

fn gen_block_quote(block_quote: &BlockQuote, context: &mut Context) -> PrintItems {
  context.mark_in_block_quotes(|context, block_quote_count| {
    let mut items = PrintItems::new();

    // add a > for any string that is on the start of a line
    // Note: This is extremely hacky
    let mut indent_level = 0;
    for print_item in gen_nodes(&block_quote.children, context).iter() {
      match print_item {
        PrintItem::String(text) => {
          items.push_condition(if_true(
            "angleBracketIfStartOfLine",
            condition_resolvers::is_start_of_line(),
            {
              let mut items = PrintItems::new();
              items.push_optional_path(context.get_memoized_rc_path(MemoizedRcPathKind::FinishIndent(indent_level)));
              items.push_string(">".repeat(block_quote_count));
              items.push_space();
              items.push_optional_path(
                context.get_memoized_rc_path(MemoizedRcPathKind::StartWithSingleIndent(indent_level)),
              );
              items
            },
          ));
          items.push_item(PrintItem::String(text));
        }
        PrintItem::Signal(Signal::NewLine) => {
          items.push_condition(if_true(
            "angleBracketIfStartOfLine",
            condition_resolvers::is_start_of_line(),
            {
              let mut items = PrintItems::new();
              items.push_optional_path(context.get_memoized_rc_path(MemoizedRcPathKind::FinishIndent(indent_level)));
              items.push_string(">".repeat(block_quote_count));
              items.push_optional_path(context.get_memoized_rc_path(MemoizedRcPathKind::StartIndent(indent_level)));
              items
            },
          ));
          items.push_signal(Signal::NewLine);
        }
        PrintItem::Signal(Signal::StartIndent | Signal::QueueStartIndent) => {
          indent_level += 1;
          items.push_item(print_item)
        }
        PrintItem::Signal(Signal::FinishIndent) => {
          indent_level -= 1;
          items.push_item(print_item)
        }
        _ => items.push_item(print_item),
      }
    }

    items
  })
}

fn gen_code_block(code_block: &CodeBlock, context: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();
  let code_text = get_code_text(code_block, context);
  let code_text = utils::unindent(code_text.trim_end());
  let backtick_text = "`".repeat(get_backtick_count(&code_text));
  let indent_level = if code_block.is_fenced { 0 } else { 4 };

  // header
  if code_block.is_fenced {
    items.push_string(backtick_text.clone());
    if let Some(tag) = &code_block.tag {
      items.push_string(tag.to_string());
    }
    items.push_signal(Signal::NewLine);
  }

  // body
  if !code_text.is_empty() {
    items.extend(ir_helpers::gen_from_string(&code_text));
  }

  // footer
  if code_block.is_fenced {
    if !code_text.is_empty() {
      items.push_signal(Signal::NewLine);
    }
    items.push_string(backtick_text);
  }

  return with_indent_times(items, indent_level);

  fn get_code_text<'a>(code_block: &'a CodeBlock, context: &mut Context) -> Cow<'a, str> {
    let code = &code_block.code;
    if code.trim().is_empty() {
      return Cow::Borrowed("");
    }
    let start_pos = get_code_block_start_pos(code);
    let code = code[start_pos..].trim_end();
    if let Some(tag) = &code_block.tag {
      // allow situations like ```rust,ignore
      let tag = tag.chars().take_while(|&c| c != ' ' && c != ',').collect::<String>();
      if let Ok(Some(text)) = context.format_text(&tag, code) {
        return Cow::Owned(text);
      }
    }
    Cow::Borrowed(code)
  }

  fn get_code_block_start_pos(text: &str) -> usize {
    let mut start_pos = 0;
    for (i, c) in text.char_indices() {
      if c == '\n' {
        start_pos = i + 1;
      } else if !c.is_whitespace() {
        break;
      }
    }
    start_pos
  }

  fn get_backtick_count(text: &str) -> usize {
    // need to count how many consecutive backticks there are in the text
    let mut count = 0;
    let mut max_count = 0;
    for c in text.chars() {
      match c {
        '`' => {
          count += 1;
          max_count = std::cmp::max(count, max_count);
        }
        _ => count = 0,
      }
    }
    std::cmp::max(2, max_count) + 1
  }
}

fn gen_code(code: &Code, _: &mut Context) -> PrintItems {
  let text = code.code.trim();
  let mut backtick_text = "`";
  let mut separator = "";
  if text.contains('`') {
    backtick_text = "``";
    if text.starts_with('`') || text.ends_with('`') {
      separator = " ";
    }
  }

  format!("{0}{1}{2}{1}{0}", backtick_text, separator, text).into()
}

fn gen_text(text: &Text, context: &mut Context) -> PrintItems {
  gen_str(&text.text, context)
}

fn is_callout_text(text: &str) -> bool {
  // ex. [!NOTE]
  text.starts_with("[!") && text.ends_with("]") && text[2..text.len() - 1].chars().all(|c| c.is_ascii_uppercase())
}

fn gen_str(text: &str, context: &mut Context) -> PrintItems {
  let mut text_builder = TextBuilder::new(context);

  for c in text.chars() {
    text_builder.add_char(c);
  }

  return text_builder.build();

  struct TextBuilder<'a> {
    items: PrintItems,
    was_last_newline: bool,
    current_word: Option<String>,
    context: &'a Context<'a>,
  }

  impl<'a> TextBuilder<'a> {
    pub fn new(context: &'a Context) -> TextBuilder<'a> {
      TextBuilder {
        items: PrintItems::new(),
        was_last_newline: false,
        current_word: None,
        context,
      }
    }

    pub fn build(mut self) -> PrintItems {
      self.flush_current_word();
      self.items
    }

    pub fn add_char(&mut self, character: char) {
      if character == '\n' || character == ' ' {
        if self.context.configuration.text_wrap == TextWrap::Maintain && character == '\n' {
          self.newline();
        } else {
          self.space_or_newline();
        }
        return;
      }

      if let Some(current_word) = self.current_word.as_mut() {
        current_word.push(character);
      } else {
        let mut text = String::new();
        text.push(character);
        self.current_word = Some(text);
      }
    }

    fn space_or_newline(&mut self) {
      self.flush_current_word();
    }

    fn newline(&mut self) {
      self.flush_current_word();
      self.was_last_newline = true;
    }

    fn flush_current_word(&mut self) {
      if let Some(current_word) = self.current_word.take() {
        if !self.items.is_empty() {
          if utils::is_list_word(&current_word) {
            self.items.push_space();
          } else if self.was_last_newline {
            self.items.push_signal(Signal::NewLine)
          } else {
            self.items.extend(get_space_or_newline_based_on_config(self.context));
          }
        }

        self.items.push_string(current_word);
        self.was_last_newline = false;
      }
    }
  }
}

fn gen_text_decoration(text: &TextDecoration, context: &mut Context) -> PrintItems {
  /// GitHub doesn't make `_` and `__` as being a text decoration when the character
  /// after the underscore is alphanumeric. For example: `__word__something`. Due
  /// to this, we need to keep the asterisk when configured for underscores
  /// in order to ensure the text keeps its meaning on GitHub.
  fn keep_asterisk(pos: usize, context: &Context) -> bool {
    &context.file_text[pos - 1..pos] == "*"
      && context.file_text[pos..]
        .chars()
        .next()
        .map(|c| c.is_alphanumeric())
        .unwrap_or(false)
  }

  let mut items = PrintItems::new();
  let decoration_text = match &text.kind {
    TextDecorationKind::Emphasis => match context.configuration.emphasis_kind {
      EmphasisKind::Asterisks => sc!("*"),
      EmphasisKind::Underscores => {
        if keep_asterisk(text.range.end, context) {
          sc!("*")
        } else {
          sc!("_")
        }
      }
    },
    TextDecorationKind::Strong => match context.configuration.strong_kind {
      StrongKind::Asterisks => sc!("**"),
      StrongKind::Underscores => {
        if keep_asterisk(text.range.end, context) {
          sc!("**")
        } else {
          sc!("__")
        }
      }
    },
    TextDecorationKind::Strikethrough => sc!("~~"),
  };

  items.push_sc(decoration_text);
  items.extend(gen_nodes(&text.children, context));
  items.push_sc(decoration_text);

  items
}

fn gen_html(node: &Html, ctx: &mut Context) -> PrintItems {
  gen_range(node.range.clone(), ctx)
}

fn gen_display_math(node: &DisplayMath, ctx: &mut Context) -> PrintItems {
  gen_range(node.range.clone(), ctx)
}

fn gen_inline_math(node: &InlineMath, ctx: &mut Context) -> PrintItems {
  gen_range(node.range.clone(), ctx)
}

fn gen_range(range: Range, ctx: &mut Context) -> PrintItems {
  let text = ctx.file_text[range].trim_end();
  if text.is_empty() {
    return PrintItems::new();
  }
  let mut items = PrintItems::new();
  items.push_sc(sc!("")); // force first line indentation
  items.extend(ir_helpers::gen_from_raw_string_trim_line_ends(text));
  items
}

fn gen_footnote_reference(footnote_reference: &FootnoteReference, _: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();
  items.push_string(format!("[^{}]", footnote_reference.name.trim()));
  ir_helpers::with_no_new_lines(items)
}

fn gen_footnote_definition(footnote_definition: &FootnoteDefinition, context: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();
  items.push_string(format!("[^{}]: ", footnote_definition.name.trim()));
  items.extend(with_indent_times(gen_nodes(&footnote_definition.children, context), 4));
  items
}

fn gen_inline_link(link: &InlineLink, context: &mut Context) -> PrintItems {
  context.with_no_text_wrap(|context| {
    let mut items = PrintItems::new();
    let generated_children = gen_nodes(&link.children, context);
    items.push_sc(sc!("["));

    // force the text to be on a single line in some scenarios
    let (generated_children, generated_children_clone) = clone_items(generated_children);
    let single_line_text = get_items_text(ir_helpers::with_no_new_lines(generated_children_clone));
    if single_line_text.len() < (context.configuration.line_width / 2) as usize {
      items.push_string(single_line_text);
    } else {
      items.extend(generated_children);
    }

    items.push_sc(sc!("]"));
    items.push_sc(sc!("("));
    items.push_string(link.url.trim().to_string());
    if let Some(title) = &link.title {
      items.push_string(format!(" \"{}\"", title.trim()));
    }
    items.push_sc(sc!(")"));

    ir_helpers::new_line_group(items)
  })
}

fn gen_reference_link(link: &ReferenceLink, context: &mut Context) -> PrintItems {
  context.with_no_text_wrap(|context| {
    let mut items = PrintItems::new();
    items.push_sc(sc!("["));
    items.extend(gen_nodes(&link.children, context));
    items.push_sc(sc!("]"));
    items.push_string(format!("[{}]", link.reference.trim()));
    ir_helpers::new_line_group(items)
  })
}

fn gen_shortcut_link(link: &ShortcutLink, context: &mut Context) -> PrintItems {
  context.with_no_text_wrap(|context| {
    let mut items = PrintItems::new();
    items.push_sc(sc!("["));
    items.extend(gen_nodes(&link.children, context));
    items.push_sc(sc!("]"));
    ir_helpers::new_line_group(items)
  })
}

fn gen_auto_link(link: &AutoLink, context: &mut Context) -> PrintItems {
  // auto-links can't contain spaces, but do this anyway just in case
  context.with_no_text_wrap(|context| {
    let mut items = PrintItems::new();
    items.push_sc(sc!("<"));
    items.extend(gen_nodes(&link.children, context));
    items.push_sc(sc!(">"));
    ir_helpers::new_line_group(items)
  })
}

fn gen_link_reference(link_ref: &LinkReference, _: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();
  items.push_string(format!("[{}]: ", link_ref.name.trim()));
  items.push_string(link_ref.link.trim().to_string());
  if let Some(title) = &link_ref.title {
    items.push_string(format!(" \"{}\"", title.trim()));
  }
  ir_helpers::new_line_group(items)
}

fn gen_inline_image(image: &InlineImage, _: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();
  items.push_string(format!("![{}]", image.text.trim()));
  items.push_sc(sc!("("));
  items.push_string(image.url.trim().to_string());
  if let Some(title) = &image.title {
    items.push_string(format!(" \"{}\"", title.trim()));
  }
  items.push_sc(sc!(")"));
  ir_helpers::new_line_group(items)
}

fn gen_reference_image(image: &ReferenceImage, _: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();
  items.push_string(format!("![{}]", image.text.trim()));
  items.push_string(format!("[{}]", image.reference.trim()));
  ir_helpers::new_line_group(items)
}

fn gen_list(list: &List, is_alternate: bool, context: &mut Context) -> PrintItems {
  context.mark_in_list(|context| {
    let mut items = PrintItems::new();

    // generate items
    for (index, child) in list.children.iter().enumerate() {
      if index > 0 {
        items.push_signal(Signal::NewLine);
        if utils::has_leading_blankline(child.range().start, context.file_text) {
          items.push_signal(Signal::NewLine);
        }
      }
      let prefix_text = if let Some(start_index) = list.start_index {
        let end_char = if is_alternate { ")" } else { "." };
        let display_index = if is_all_ones_list(list, context) {
          1
        } else {
          start_index + index as u64
        };
        format!("{}{}", display_index, end_char)
      } else {
        String::from(context.configuration.unordered_list_kind.list_char(is_alternate))
      };
      let indent_increment = (prefix_text.chars().count() + 1) as u32;
      context.indent_level += indent_increment;
      items.push_string(prefix_text);
      let after_child = LineAndColumn::new("afterChild");
      items.push_condition(if_true(
        "spaceIfHasChild",
        Rc::new(move |context| Some(!condition_helpers::is_at_same_position(context, after_child)?)),
        Signal::SpaceIfNotTrailing.into(),
      ));
      items.extend(with_indent_times(generate(child, context), indent_increment));
      items.push_line_and_column(after_child);
      context.indent_level -= indent_increment;
    }

    items
  })
}

fn gen_item(item: &Item, context: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();

  if let Some(marker) = &item.marker {
    items.extend(gen_task_list_marker(marker, context));
    if !item.children.is_empty() {
      items.push_space();
    }
  }

  items.extend(gen_task_list_marker_children(
    &item.children,
    item.marker.as_ref(),
    context,
  ));

  if !item.sub_lists.is_empty() {
    items.push_signal(Signal::NewLine);
    if utils::has_leading_blankline(item.sub_lists.first().unwrap().range().start, context.file_text) {
      items.push_signal(Signal::NewLine);
    }
    items.extend(gen_nodes(&item.sub_lists, context));
  }

  items
}

fn gen_task_list_marker_children(
  children: &[Node],
  marker: Option<&TaskListMarker>,
  context: &mut Context,
) -> PrintItems {
  let mut items = PrintItems::new();
  // indent the children to beyond the task list marker
  let marker_indent = if marker.is_some() { 4 } else { 0 };
  context.raw_indent_level += marker_indent;
  let indent_child_index_end = children
    .iter()
    .position(|c| {
      matches!(
        c,
        Node::List(_) | Node::CodeBlock(_) | Node::BlockQuote(_) | Node::Heading(_) | Node::Table(_)
      ) || utils::has_leading_blankline(c.range().start, context.file_text)
    })
    .unwrap_or(children.len());
  items.extend(with_indent_times(
    gen_nodes(&children[..indent_child_index_end], context),
    marker_indent,
  ));
  context.raw_indent_level -= marker_indent;

  // insert the remaining children without indent
  if indent_child_index_end > 0 && indent_child_index_end != children.len() {
    items.push_signal(Signal::NewLine);
    if utils::has_leading_blankline(children[indent_child_index_end].range().start, context.file_text) {
      items.push_signal(Signal::NewLine);
    }
  }
  items.extend(gen_nodes(&children[indent_child_index_end..], context));
  items
}

fn gen_task_list_marker(marker: &TaskListMarker, _: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();
  if marker.is_checked {
    items.push_string("[x]".into());
  } else {
    items.push_string("[ ]".into());
  }

  items
}

fn gen_horizontal_rule(_: &HorizontalRule, _: &mut Context) -> PrintItems {
  "---".into()
}

fn gen_hard_break(_: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();
  items.push_sc(sc!("\\"));
  items.push_signal(Signal::NewLine);
  items
}

fn gen_table(table: &Table, context: &mut Context) -> PrintItems {
  // Keep table output structurally consistent instead of visually column-aligned.
  // This emits exactly one delimiter-adjacent space around each non-empty cell.
  let header = table
    .header
    .cells
    .iter()
    .map(|cell| gen_table_cell(cell, context))
    .collect::<Vec<_>>();
  let rows = table
    .rows
    .iter()
    .map(|row| {
      row
        .cells
        .iter()
        .map(|cell| gen_table_cell(cell, context))
        .collect::<Vec<_>>()
    })
    .collect::<Vec<_>>();
  let column_count = get_column_count(&header, &rows, &table.column_alignment);
  let mut items = PrintItems::new();

  items.extend(get_row_items(header));
  items.push_signal(Signal::NewLine);
  items.extend(get_divider_row(column_count, &table.column_alignment));

  for row in rows {
    items.push_signal(Signal::NewLine);
    items.extend(get_row_items(row));
  }

  return items;

  fn get_divider_row(column_count: usize, column_alignments: &[ColumnAlignment]) -> PrintItems {
    let mut items = PrintItems::new();
    for i in 0..column_count {
      let column_alignment = column_alignments.get(i).copied().unwrap_or(ColumnAlignment::None);
      if i == 0 {
        items.push_sc(sc!("| "));
      } else {
        items.push_space();
      }

      items.push_sc(get_column_alignment_text(column_alignment));
      items.push_sc(sc!(" |"));
    }

    ir_helpers::with_no_new_lines(items)
  }

  fn get_row_items(row_cells: Vec<PrintItems>) -> PrintItems {
    let mut items = PrintItems::new();
    for (i, cell_items) in row_cells.into_iter().enumerate() {
      if i == 0 {
        items.push_sc(sc!("| "))
      } else {
        items.push_space();
      }

      items.extend(cell_items);
      items.push_sc(sc!(" |"));
    }

    ir_helpers::with_no_new_lines(items)
  }

  fn get_column_count(header: &[PrintItems], rows: &[Vec<PrintItems>], column_alignments: &[ColumnAlignment]) -> usize {
    let row_column_count = rows.iter().map(|row| row.len()).max().unwrap_or(0);
    header.len().max(row_column_count).max(column_alignments.len())
  }

  fn get_column_alignment_text(column_alignment: ColumnAlignment) -> &'static StringContainer {
    match column_alignment {
      ColumnAlignment::None => sc!("---"),
      ColumnAlignment::Left => sc!(":---"),
      ColumnAlignment::Center => sc!(":---:"),
      ColumnAlignment::Right => sc!("---:"),
    }
  }
}

fn gen_table_cell(table_cell: &TableCell, context: &mut Context) -> PrintItems {
  gen_nodes(&table_cell.children, context)
}

fn gen_metadata_block(node: &MetadataBlock, context: &mut Context) -> PrintItems {
  let mut items = PrintItems::new();

  let delimiter = match node.kind {
    MetadataBlockKind::YamlStyle => sc!("---"),
    MetadataBlockKind::PlusesStyle => sc!("+++"),
  };

  items.push_sc(delimiter);
  items.push_signal(Signal::NewLine);
  match node.kind {
    MetadataBlockKind::YamlStyle => {
      let text = context
        .format_text("yaml", &node.text)
        .ok()
        .flatten()
        .map(Cow::from)
        .unwrap_or_else(|| Cow::from(&node.text));
      items.extend(ir_helpers::gen_from_string_trim_line_ends(text.trim_end()));
    }
    MetadataBlockKind::PlusesStyle => {
      items.extend(ir_helpers::gen_from_raw_string_trim_line_ends(node.text.trim_end()));
    }
  }
  items.push_signal(Signal::NewLine);
  items.push_sc(delimiter);

  items
}

fn clone_items(items: PrintItems) -> (PrintItems, PrintItems) {
  // todo: something in the core library? This is weird
  let rc_path = items.into_rc_path();
  let mut items1 = PrintItems::new();
  let mut items2 = PrintItems::new();
  items1.push_optional_path(rc_path);
  items2.push_optional_path(rc_path);
  (items1, items2)
}

fn get_items_text(items: PrintItems) -> String {
  print(
    ir_helpers::with_no_new_lines(items),
    PrintOptions {
      indent_width: 0,
      max_width: u32::MAX,
      use_tabs: false,
      new_line_text: "",
    },
  )
}

fn measure_longest_line_width(items: PrintItems, max_width: u32) -> usize {
  let rendered = print(
    items,
    PrintOptions {
      indent_width: 0,
      max_width,
      use_tabs: false,
      new_line_text: "\n",
    },
  );
  rendered.lines().map(UnicodeWidthStr::width).max().unwrap_or(0)
}

fn get_space_or_newline_based_on_config(context: &Context) -> PrintItems {
  if context.is_text_wrap_disabled() {
    return space();
  }
  match context.configuration.text_wrap {
    TextWrap::Always => Signal::SpaceOrNewLine.into(),
    TextWrap::Never | TextWrap::Maintain => space(),
  }
}

fn space() -> PrintItems {
  let mut items = PrintItems::new();
  items.push_space();
  items
}

fn get_newline_wrapping_based_on_config(context: &Context) -> PrintItems {
  match context.configuration.text_wrap {
    TextWrap::Always => Signal::SpaceOrNewLine.into(),
    TextWrap::Never => space(),
    TextWrap::Maintain => {
      if context.is_text_wrap_disabled() {
        if_true_or(
          "newLineOrSpaceIfNewlinesDisabled",
          condition_resolvers::is_forcing_no_newlines(),
          space(),
          Signal::NewLine.into(),
        )
        .into()
      } else {
        Signal::NewLine.into()
      }
    }
  }
}

/// If the list's first items are both 1s
fn is_all_ones_list(list: &List, context: &Context) -> bool {
  list.children.len() > 1 && list.start_index.unwrap_or(0) == 1 && {
    let text = list.children.get(1).unwrap().text(context).trim();
    text.starts_with("1.") || text.starts_with("1)")
  }
}
