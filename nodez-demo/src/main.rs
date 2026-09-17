//! A visual generator for container-stack config files, built on `nodez`.
//!
//! The editor window itself comes from [`nodez::app::EditorApp`]; everything
//! here is domain: the node library, the sample graph, and the rule that turns
//! a graph into a document.

#![forbid(unsafe_code)]

mod domain;
mod generate;
mod sample;
mod yaml;

use nodez::EditorStyle;
use nodez::app::{EditorApp, Preview};

use domain::Domain;

fn main() -> eframe::Result {
    let domain = Domain::new();
    let style = EditorStyle::blender_dark();
    let graph = sample::build(&domain, &style);

    // `--print` generates the sample stack's config and exits, so the emitter
    // can be exercised without a display.
    if std::env::args().any(|arg| arg == "--print") {
        let generated = generate::generate(&graph, &domain.library);
        for problem in &generated.problems {
            eprintln!("warning: {problem}");
        }
        print!("{}", generated.text);
        return Ok(());
    }

    EditorApp::new(domain.library)
        .graph(graph)
        .style(style)
        .title("nodez \u{2014} stack config editor")
        .file("stack-graph.json")
        .preview_extension("yaml")
        .json_files()
        .preview(|graph, library| {
            let generated = generate::generate(graph, library);
            Preview::text(generated.text)
                .problems(generated.problems)
                .contributing(generated.contributing)
        })
        .run()
}
