extern crate dprint_development;
extern crate dprint_plugin_markdown;

use std::path::PathBuf;
use std::sync::Arc;

use dprint_core::configuration::*;
use dprint_development::*;
use dprint_plugin_markdown::configuration::*;
use dprint_plugin_markdown::*;

fn main() {
  //debug_here!();
  let global_config = GlobalConfiguration::default();

  run_specs(
    &PathBuf::from("./tests/specs"),
    &ParseSpecOptions {
      default_file_name: "file.md",
    },
    &RunSpecsOptions {
      fix_failures: false,
      format_twice: true,
    },
    {
      let global_config = global_config.clone();
      Arc::new(move |file_path, file_text, spec_config| {
        let spec_config: ConfigKeyMap = serde_json::from_value(spec_config.clone().into()).unwrap();
        let config_result = resolve_config(spec_config, &global_config);
        ensure_no_diagnostics(&config_result.diagnostics);

        format_text_for_file_path(
          file_path,
          file_text,
          &config_result.config,
          |tag, file_text, line_width| {
            let end = format!("_formatted_{}", line_width);
            if tag == "format" && !file_text.ends_with(&end) {
              Ok(Some(format!("{}{}\n\n", file_text, end)))
            } else if tag == "tsx" && file_text.contains("format_mdx_esm_unsafe") {
              Ok(Some("import { A, B } from \"x\"\n\n# injected markdown\n".to_string()))
            } else if tag == "tsx" && file_text.contains("format_mdx_esm") {
              Ok(Some(file_text.replace("import {A,B} from", "import { A, B } from")))
            } else if tag == "tsx" && file_text.contains("format_mdx_jsx_inject_markdown") {
              Ok(Some(
                "<Component format_mdx_jsx_inject_markdown />\n\n# injected markdown\n\n<Other />\n".to_string(),
              ))
            } else if tag == "tsx" && file_text.contains("format_mdx_jsx_unsafe") {
              Ok(Some("<Component format_mdx_jsx_unsafe={true}".to_string()))
            } else if tag == "tsx" && file_text.contains("format_mdx_jsx_complex") {
              Ok(Some(
                "<Foo format_mdx_jsx_complex=\"yes\">\n  <Bar>hi</Bar>\n  {hello}\n  {/* another comment */}\n</Foo>\n"
                  .to_string(),
              ))
            } else if tag == "tsx" && file_text.contains("format_mdx_jsx") {
              Ok(Some(file_text.replace(
                "<Component format_mdx_jsx={true}></Component>",
                "<Component format_mdx_jsx={true} />",
              )))
            } else if (tag == "yaml" || tag == "toml") && file_text.contains("format_metadata") {
              let end = format!("# {}_metadata_formatted_{}", tag, line_width);
              if file_text.contains(&end) {
                Ok(None)
              } else {
                Ok(Some(format!("{}\n{}\n", file_text.trim_end(), end)))
              }
            } else {
              Ok(None)
            }
          },
        )
      })
    },
    Arc::new(move |_, _file_text, _spec_config| {
      #[cfg(feature = "tracing")]
      {
        let spec_config: ConfigKeyMap = serde_json::from_value(_spec_config.clone().into()).unwrap();
        let config_result = resolve_config(spec_config, &global_config);
        ensure_no_diagnostics(&config_result.diagnostics);
        serde_json::to_string(&trace_file(
          _file_text,
          &config_result.config,
          |tag, file_text, line_width| {
            let end = format!("_formatted_{}", line_width);
            if tag == "format" && !file_text.ends_with(&end) {
              Ok(Some(format!("{}{}\n\n", file_text, end)))
            } else if (tag == "yaml" || tag == "toml") && file_text.contains("format_metadata") {
              let end = format!("# {}_metadata_formatted_{}", tag, line_width);
              if file_text.contains(&end) {
                Ok(None)
              } else {
                Ok(Some(format!("{}\n{}\n", file_text.trim_end(), end)))
              }
            } else {
              Ok(None)
            }
          },
        ))
        .unwrap()
      }

      #[cfg(not(feature = "tracing"))]
      panic!("\n====\nPlease run with `cargo test --features tracing` to get trace output\n====\n")
    }),
  );
}
