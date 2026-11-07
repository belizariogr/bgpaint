//! Tools and their settings.

use bgpaint_core::brush::BrushKind;
use bgpaint_core::shapes::{ShapeKind, ShapeStyle};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tool {
    Select,
    Pencil,
    Fill,
    Text,
    Eraser,
    Picker,
    Magnifier,
    Brush,
    Shape,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SelectMode {
    Rectangle,
    FreeForm,
}

/// Size and opacity of a family of tools.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub size: f32,
    pub opacity: f32,
}

#[derive(Clone, Debug)]
pub struct ToolSettings {
    pub brush: BrushKind,
    pub shape: ShapeKind,
    pub outline: ShapeStyle,
    pub fill: ShapeStyle,
    pub select_mode: SelectMode,
    pub transparent_selection: bool,
    pub pencil: Stroke,
    pub brush_stroke: Stroke,
    pub eraser: Stroke,
    /// Outline width and opacity of shapes (the side panel's sliders).
    pub shape_stroke: Stroke,
}

impl Default for ToolSettings {
    fn default() -> Self {
        Self {
            brush: BrushKind::Brush,
            shape: ShapeKind::Rectangle,
            outline: ShapeStyle::Solid,
            fill: ShapeStyle::None,
            select_mode: SelectMode::Rectangle,
            transparent_selection: false,
            pencil: Stroke { size: 1.0, opacity: 1.0 },
            brush_stroke: Stroke { size: 8.0, opacity: 1.0 },
            eraser: Stroke { size: 12.0, opacity: 1.0 },
            shape_stroke: Stroke { size: 5.0, opacity: 1.0 },
        }
    }
}

pub const MAX_SIZE: f32 = 100.0;

impl ToolSettings {
    /// The size/opacity pair edited by the side panel for `tool`, if it has one.
    pub fn stroke_mut(&mut self, tool: Tool) -> Option<&mut Stroke> {
        match tool {
            Tool::Pencil => Some(&mut self.pencil),
            Tool::Brush => Some(&mut self.brush_stroke),
            Tool::Eraser => Some(&mut self.eraser),
            Tool::Shape => Some(&mut self.shape_stroke),
            _ => None,
        }
    }

    pub fn stroke(&self, tool: Tool) -> Option<Stroke> {
        match tool {
            Tool::Pencil => Some(self.pencil),
            Tool::Brush => Some(self.brush_stroke),
            Tool::Eraser => Some(self.eraser),
            Tool::Shape => Some(self.shape_stroke),
            _ => None,
        }
    }
}

pub mod interaction;
pub mod paint;
pub mod select;
pub mod shape;
pub mod text;
pub mod transform;
