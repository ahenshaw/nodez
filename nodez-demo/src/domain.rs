//! Assembling the library: the node kinds from [`crate::nodes`], the colours
//! the demo wants, and the implicit conversions between socket types.

use egui::Color32;
use nodez::{NodeLibrary, Rules};

use crate::nodes::{Config, Nodes};

// Blender's own header palette, which is the point of overriding the derived
// colours here: the demo is deliberately imitating a particular look.
const INPUT: Color32 = Color32::from_rgb(0x3B, 0x52, 0x70);
const CONVERT: Color32 = Color32::from_rgb(0x4A, 0x5F, 0x3B);
const BUILD: Color32 = Color32::from_rgb(0x6E, 0x5A, 0x2E);
const RUNTIME: Color32 = Color32::from_rgb(0x7A, 0x4A, 0x2E);
const OUTPUT: Color32 = Color32::from_rgb(0x5C, 0x3B, 0x5C);

/// The library and the rules for folding a graph into a config document.
pub fn build() -> (NodeLibrary, Rules<Config>) {
    let mut library = NodeLibrary::new();
    let mut rules = Rules::<Config>::new();
    rules.register_all::<Nodes>(&mut library);

    for (category, color) in [
        ("Input", INPUT),
        ("Convert", CONVERT),
        ("Build", BUILD),
        ("Runtime", RUNTIME),
        ("Output", OUTPUT),
    ] {
        library.set_category_color(category, color);
    }

    // Numbers and flags can be spelled as text; text cannot become either.
    // This is what makes a Number socket droppable onto a Text input.
    let types = &mut library.types;
    if let (Some(text), Some(int), Some(flag)) = (types.id("Text"), types.id("Int"), types.id("Bool"))
    {
        types.allow_cast(int, text);
        types.allow_cast(flag, text);
    }

    (library, rules)
}
