//! Core engine of BGPaint: the layered document, undo history, raster operations, brushes,
//! shapes, text and file formats. It has no UI dependencies and is fully unit tested.
#![forbid(unsafe_code)]

pub mod blend;
pub mod brush;
pub mod color;
pub mod document;
pub mod fill;
pub mod geom;
pub mod history;
pub mod io;
pub mod noise;
pub mod ora;
pub mod patch;
pub mod png;
pub mod raster;
pub mod selection;
pub mod shapes;
pub mod text;
pub mod vector;
pub(crate) mod vector_xml;

pub use cosmic_text;
pub use tiny_skia;
