//! A visual generator for container-stack config files, built on `nodez`.
//!
//! The editor window comes from [`nodez::app::EditorApp`]; everything here is
//! domain. [`nodes`] describes the node kinds as Rust types, [`generate`] folds
//! a graph into a document, and [`yaml`] spells it out.

#![forbid(unsafe_code)]

mod domain;
mod generate;
mod nodes;
mod sample;
mod yaml;

use nodez::EditorStyle;
use nodez::app::{EditorApp, Preview};

fn main() -> eframe::Result {
    let (library, rules) = domain::build();
    let style = EditorStyle::blender_dark();
    let graph = sample::build(&library, &style);

    // `--print` generates the sample stack's config and exits, so the emitter
    // can be exercised without a display.
    if std::env::args().any(|arg| arg == "--print") {
        let generated = generate::generate(&graph, &library, &rules);
        for problem in &generated.problems {
            eprintln!("warning: {problem}");
        }
        print!("{}", generated.text);
        return Ok(());
    }

    EditorApp::new(library)
        .graph(graph)
        .style(style)
        .title("nodez \u{2014} stack config editor")
        .file("stack-graph.json")
        .preview_extension("yaml")
        .json_files()
        .preview(move |graph, library| {
            let generated = generate::generate(graph, library, &rules);
            Preview::text(generated.text)
                .problems(generated.problems)
                .contributing(generated.contributing)
        })
        .run()
}
