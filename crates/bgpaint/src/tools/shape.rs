//! The Shapes tool. A shape stays editable (move, resize, rotate, restyle) until the user clicks
//! outside of it or switches tools, exactly like Paint. In vector mode it then becomes a vector
//! object on the layer, which a later click picks up for editing again.

use super::interaction::Active;
use super::transform::{BoxGeom, BoxHit};
use crate::app::PaintApp;
use crate::canvas::view::View;
use crate::theme::Theme;
use crate::ui::widgets;
use bgpaint_core::document::{ActivePreview, DocState, LayerId};
use bgpaint_core::geom::{PixelRect, Point, RectF};
use bgpaint_core::history::Change;
use bgpaint_core::raster;
use bgpaint_core::shapes::{self, RasterShape, ShapeKind, ShapePaint};
use bgpaint_core::text::{self, TextStyle};
use bgpaint_core::tiny_skia::Pixmap;
use bgpaint_core::vector::{ShapeGeom, VectorShape};
use egui::{Modifiers, PointerButton, Pos2};
use std::time::Instant;

/// Geometry of the shape being edited.
#[derive(Clone, Debug)]
pub enum Geom {
    /// Any shape drawn inside a (possibly rotated) box. Free polygons keep their points
    /// normalized to the box.
    Box {
        kind: ShapeKind,
        geom: BoxGeom,
        points: Option<Vec<(f32, f32)>>,
    },
    Line {
        a: Point,
        b: Point,
    },
    /// Cubic curve; `stage` counts the bends applied so far (0, 1 or 2).
    Curve {
        a: Point,
        b: Point,
        c1: Point,
        c2: Point,
        stage: u8,
    },
    /// Arrow through `points` (rounded corners, head at the last point). While `drawing`, the
    /// last point is the tip following the pointer (also with no button held); each click fixes
    /// it as a corner and a new tip goes on following, for as many corners as wanted. A right
    /// click (or Escape) ends the drawing, dropping the leg that follows the pointer unless it is
    /// being dragged.
    Arrow {
        points: Vec<Point>,
        drawing: bool,
    },
    /// Polygon being clicked point by point.
    PolygonDraft {
        points: Vec<Point>,
    },
    /// A text object picked up to be moved, rotated or re-wrapped (a double click edits its
    /// words, with the Text tool).
    Text {
        geom: BoxGeom,
        text: String,
        style: TextStyle,
        auto_size: bool,
    },
}

impl Geom {
    /// The same geometry moved by (`dx`,`dy`).
    fn translated(&self, dx: f32, dy: f32) -> Geom {
        match self {
            Geom::PolygonDraft { points } => Geom::PolygonDraft { points: points.iter().map(|p| Point::new(p.x + dx, p.y + dy)).collect() },
            Geom::Curve { a, b, c1, c2, stage } => {
                let t = |p: &Point| Point::new(p.x + dx, p.y + dy);
                Geom::Curve { a: t(a), b: t(b), c1: t(c1), c2: t(c2), stage: *stage }
            }
            Geom::Arrow { points, drawing } => {
                Geom::Arrow { points: points.iter().map(|p| Point::new(p.x + dx, p.y + dy)).collect(), drawing: *drawing }
            }
            other => other.to_shape().map_or_else(|| other.clone(), |g| Geom::from_shape(&g.translated(dx, dy))),
        }
    }

    /// The finished geometry (`None` for a polygon still being clicked).
    fn to_shape(&self) -> Option<ShapeGeom> {
        Some(match self {
            Geom::Box { kind, geom, points } => ShapeGeom::Box { kind: *kind, rect: geom.rect, angle: geom.angle, points: points.clone() },
            Geom::Line { a, b } => ShapeGeom::Line { a: *a, b: *b },
            Geom::Curve { a, b, c1, c2, .. } => ShapeGeom::Curve { a: *a, b: *b, c1: *c1, c2: *c2 },
            Geom::Arrow { points, .. } => ShapeGeom::Arrow { points: points.clone() },
            Geom::Text { geom, text, style, auto_size } => {
                ShapeGeom::Text { rect: geom.rect, angle: geom.angle, text: text.clone(), style: style.clone(), auto_size: *auto_size }
            }
            Geom::PolygonDraft { .. } => return None,
        })
    }

    /// Editable geometry for a vector object (a curve comes back with both bends done).
    fn from_shape(g: &ShapeGeom) -> Geom {
        match g {
            ShapeGeom::Box { kind, rect, angle, points } => {
                Geom::Box { kind: *kind, geom: BoxGeom { rect: *rect, angle: *angle }, points: points.clone() }
            }
            ShapeGeom::Line { a, b } => Geom::Line { a: *a, b: *b },
            ShapeGeom::Curve { a, b, c1, c2 } => Geom::Curve { a: *a, b: *b, c1: *c1, c2: *c2, stage: 2 },
            ShapeGeom::Arrow { points } => Geom::Arrow { points: points.clone(), drawing: false },
            ShapeGeom::Text { rect, angle, text, style, auto_size } => {
                Geom::Text { geom: BoxGeom { rect: *rect, angle: *angle }, text: text.clone(), style: style.clone(), auto_size: *auto_size }
            }
        }
    }
}

#[derive(Clone, Debug)]
enum Drag {
    Create {
        start: Point,
    },
    Box(BoxHit, Point, BoxGeom),
    Endpoint(bool),
    Bend(u8),
    /// Moving point `.0` of an arrow.
    ArrowPoint(usize),
    PolyPoint,
    /// Moving the whole shape. `at` is where the raster was when the drag started, when it can
    /// simply be shifted (it holds the whole shape, nothing clipped by the canvas edge).
    Move {
        start: Point,
        orig: Geom,
        at: Option<(i32, i32)>,
    },
}

/// A vector object taken off its layer to be edited.
pub(super) struct Lifted {
    layer: LayerId,
    index: usize,
    pub(super) original: VectorShape,
    /// The document before the object was taken (the undo step of the edit).
    pub(super) before: DocState,
}

pub struct ShapeOp {
    pub geom: Geom,
    pub paint: ShapePaint,
    /// The style the tool settings gave last. A style change only overrides the fields that
    /// changed, so resizing a picked-up object keeps its own colors.
    applied: ShapePaint,
    /// Drawn with the right button: outline uses Color 2 and fill Color 1.
    swap: bool,
    drag: Option<Drag>,
    raster: Option<RasterShape>,
    /// `raster` is out of date; it is re-rendered once per frame by [`PaintApp::flush_shape`],
    /// however many pointer moves arrived.
    stale: bool,
    /// Becomes a vector object when committed, instead of pixels.
    vector: bool,
    lifted: Option<Lifted>,
    canvas: (u32, u32),
    last_click: Option<(Instant, Point)>,
    /// The button holding the current drag; releasing another one does not end it.
    drag_button: PointerButton,
    /// An arrow point marked with a double click (Delete removes just that point).
    selected_point: Option<usize>,
}

impl ActivePreview for ShapeOp {
    fn bounds(&self) -> PixelRect {
        self.raster.as_ref().map_or(PixelRect::EMPTY, RasterShape::bounds)
    }

    fn apply(&self, region: &mut Pixmap, rect: PixelRect) {
        if let Some(r) = &self.raster {
            raster::draw_over(region, &r.pixmap, r.x - rect.x0, r.y - rect.y0, 1.0);
        }
    }

    fn above_objects(&self) -> bool {
        true
    }
}

fn snap_45(a: Point, b: Point) -> Point {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let len = (dx * dx + dy * dy).sqrt();
    let angle = dy.atan2(dx);
    let step = std::f32::consts::FRAC_PI_4;
    let snapped = (angle / step).round() * step;
    Point::new(a.x + len * snapped.cos(), a.y + len * snapped.sin())
}

fn square_from(a: Point, b: Point) -> Point {
    let (dx, dy) = (b.x - a.x, b.y - a.y);
    let s = dx.abs().max(dy.abs());
    Point::new(a.x + s * dx.signum(), a.y + s * dy.signum())
}

impl ShapeOp {
    /// Re-rasterizes after a change; returns the canvas area to redraw.
    fn update(&mut self) -> PixelRect {
        let before = self.bounds();
        if let Geom::Text { geom, text, style, auto_size: true } = &mut self.geom {
            let (w, h) = text::fit_box(text, style);
            *geom = geom.resized(w, h, style.align.anchor());
        }
        self.raster = match &self.geom {
            Geom::PolygonDraft { points } => {
                shapes::polygon_path(points, false).and_then(|p| shapes::rasterize(&p, &self.paint, false, self.canvas.0, self.canvas.1))
            }
            g => g.to_shape().and_then(|g| g.rasterize(&self.paint, self.canvas)),
        };
        self.stale = false;
        // A move in progress can no longer shift the raster it started with (it was redrawn,
        // perhaps with another size): the rest of the drag redraws it.
        if let Some(Drag::Move { at, .. }) = &mut self.drag {
            *at = None;
        }
        before.union(&self.bounds())
    }

    /// Removes the arrow point marked with a double click; the arrow is redrawn without it (the
    /// head moves to the point before a removed tip). An arrow keeps at least two points.
    /// Returns whether there was a marked point.
    pub fn delete_selected_point(&mut self) -> bool {
        let (Geom::Arrow { points, .. }, Some(i)) = (&mut self.geom, self.selected_point) else { return false };
        if points.len() > 2 && i < points.len() {
            points.remove(i);
        }
        self.selected_point = None;
        self.stale = true;
        true
    }

    /// A drag that moves the whole shape from `p`.
    fn start_move(&self, p: Point) -> Drag {
        let at = self.raster.as_ref().filter(|r| !r.clipped && !self.stale).map(|r| (r.x, r.y));
        Drag::Move { start: p, orig: self.geom.clone(), at }
    }

    fn editing(&self) -> bool {
        self.drag.is_none()
    }
}

impl PaintApp {
    /// The Shapes tool is the vector Pointer (only while vector mode is on).
    pub fn vector_pointer(&self) -> bool {
        self.tools.vector_pointer && self.doc.vector_mode()
    }

    fn shape_paint(&self, swap: bool) -> ShapePaint {
        let (outline, fill) =
            if swap { (self.colors.secondary, self.colors.primary) } else { (self.colors.primary, self.colors.secondary) };
        ShapePaint {
            outline: self.tools.outline,
            fill: self.tools.fill,
            width: self.tools.shape_stroke.size,
            outline_color: outline,
            fill_color: fill,
            opacity: self.tools.shape_stroke.opacity,
        }
    }

    pub fn shape_pointer_down(&mut self, p: Point, _pos: Pos2, button: PointerButton, _m: Modifiers) {
        let tol = 6.0 / self.view.scale();
        let rotate_off = 24.0 / self.view.scale();
        // A right click ends the drawing of an arrow (which stays editable), back to the Pointer
        // in vector mode.
        if let Active::Shape(op) = &mut self.active
            && matches!(op.geom, Geom::Arrow { drawing: true, .. })
            && button == PointerButton::Secondary
        {
            if !finish_arrow(op) {
                // Nothing fixed yet (just the first click).
                self.cancel_interaction();
                return;
            }
            if self.doc.vector_mode() {
                self.tools.vector_pointer = true;
            }
            return;
        }
        // Interact with the shape being edited.
        if let Active::Shape(op) = &mut self.active
            && button == PointerButton::Primary
        {
            if let Geom::PolygonDraft { points } = &mut op.geom {
                let close = points.len() > 2 && points.first().is_some_and(|f| f.distance(p) <= tol * 1.5);
                let double = op.last_click.is_some_and(|(t, q)| t.elapsed().as_millis() < 450 && q.distance(p) <= tol * 2.0);
                if close || double {
                    finish_polygon(op);
                    let d = op.update();
                    self.preview_dirty.add(d);
                    // The polygon is formed: back to the Pointer (in vector mode).
                    if self.doc.vector_mode() {
                        self.tools.vector_pointer = true;
                    }
                } else {
                    points.push(p);
                    op.last_click = Some((Instant::now(), p));
                    op.drag = Some(Drag::PolyPoint);
                    let d = op.update();
                    self.preview_dirty.add(d);
                }
                return;
            }
            if let Geom::Arrow { points, drawing } = &mut op.geom {
                // While drawing, a click goes on from the tip (which becomes a corner) to a new
                // tip there, dragged while the button is held. Afterwards a point's handle moves
                // that point and the line moves the whole arrow.
                let drag = if *drawing {
                    // The tip comes to the click (held, it can still be dragged); releasing fixes it
                    // and a new tip follows.
                    if let Some(tip) = points.last_mut() {
                        *tip = p;
                    }
                    op.stale = true;
                    Some(Drag::ArrowPoint(points.len() - 1))
                } else if let Some(i) = points.iter().position(|q| q.distance(p) <= tol * 1.5) {
                    // A double click on a point marks it (only when the arrow has more than two, so
                    // one can go); pressing another point drops the mark.
                    let double = op.last_click.is_some_and(|(t, q)| t.elapsed().as_millis() < 450 && q.distance(p) <= tol * 2.0);
                    if double && points.len() > 2 {
                        op.selected_point = Some(i);
                    } else if op.selected_point != Some(i) {
                        op.selected_point = None;
                    }
                    op.last_click = Some((Instant::now(), p));
                    Some(Drag::ArrowPoint(i))
                } else {
                    op.selected_point = None;
                    op.raster.as_ref().is_some_and(|r| r.hit(p, tol)).then(|| op.start_move(p))
                };
                if let Some(d) = drag {
                    op.drag = Some(d);
                    op.drag_button = button;
                    return;
                }
            }
            // A double click on a picked-up text edits its words.
            if let Geom::Text { geom, .. } = &op.geom
                && geom.hit(p, tol, rotate_off) == Some(BoxHit::Inside)
            {
                let double = op.last_click.is_some_and(|(t, q)| t.elapsed().as_millis() < 450 && q.distance(p) <= tol * 2.0);
                op.last_click = Some((Instant::now(), p));
                if double {
                    self.edit_lifted_text(p);
                    return;
                }
            }
            let hit = match &op.geom {
                Geom::Box { geom, .. } | Geom::Text { geom, .. } => match geom.hit(p, tol, rotate_off) {
                    Some(BoxHit::Inside) => Some(op.start_move(p)),
                    h => h.map(|h| Drag::Box(h, p, *geom)),
                },
                Geom::Line { a, b } => {
                    if a.distance(p) <= tol * 1.5 {
                        Some(Drag::Endpoint(false))
                    } else if b.distance(p) <= tol * 1.5 {
                        Some(Drag::Endpoint(true))
                    } else {
                        near_segment(*a, *b, p, tol + op.paint.width).then(|| op.start_move(p))
                    }
                }
                Geom::Curve { stage, .. } if *stage < 2 => Some(Drag::Bend(*stage + 1)),
                // A finished curve (picked up in vector mode) moves when grabbed.
                Geom::Curve { .. } => op.raster.as_ref().is_some_and(|r| r.hit(p, tol)).then(|| op.start_move(p)),
                _ => None,
            };
            if let Some(h) = hit {
                op.drag = Some(h);
                op.drag_button = button;
                return;
            }
        }
        self.commit_interaction();
        // In vector mode a click on an object picks it up (and starts moving it).
        if button == PointerButton::Primary
            && self.doc.vector_mode()
            && let Some((layer, index)) = self.doc.object_at(p, tol)
        {
            self.lift_object(layer, index, p);
            return;
        }
        // The Pointer only picks up objects.
        if self.vector_pointer() {
            return;
        }
        let swap = button == PointerButton::Secondary;
        let kind = self.tools.shape;
        let geom = match kind {
            ShapeKind::Line => Geom::Line { a: p, b: p },
            ShapeKind::Curve => Geom::Curve { a: p, b: p, c1: p, c2: p, stage: 0 },
            ShapeKind::CurvedArrow => Geom::Arrow { points: vec![p, p], drawing: true },
            ShapeKind::Polygon => Geom::PolygonDraft { points: vec![p, p] },
            k => Geom::Box { kind: k, geom: BoxGeom { rect: RectF::from_points(p, p), angle: 0.0 }, points: None },
        };
        let paint = self.shape_paint(swap);
        let mut op = ShapeOp {
            geom,
            paint,
            applied: paint,
            swap,
            drag: Some(Drag::Create { start: p }),
            raster: None,
            stale: false,
            vector: self.doc.vector_mode(),
            lifted: None,
            canvas: (self.doc.width(), self.doc.height()),
            last_click: Some((Instant::now(), p)),
            drag_button: button,
            selected_point: None,
        };
        let d = op.update();
        self.preview_dirty.add(d);
        self.active = Active::Shape(op);
    }

    /// Takes object `index` of layer `layer` off the document, remembering how to put it back.
    pub(super) fn take_object_for_edit(&mut self, layer: usize, index: usize) -> Option<Lifted> {
        let before = self.doc.snapshot();
        self.doc.set_active(layer);
        let layer_id = self.doc.layers()[layer].id;
        let original = self.doc.take_object(layer, index)?;
        Some(Lifted { layer: layer_id, index, original, before })
    }

    /// Font size and opacity of the text object picked up by the Pointer, if there is one.
    pub fn picked_text(&self) -> Option<(f32, f32)> {
        match &self.active {
            Active::Shape(ShapeOp { geom: Geom::Text { style, .. }, paint, .. }) => Some((style.size_pt, paint.opacity)),
            _ => None,
        }
    }

    /// Changes the picked-up text's opacity and font size; the box scales along, so the text
    /// keeps its line breaks.
    pub fn set_picked_text(&mut self, size_pt: f32, opacity: f32) {
        let Active::Shape(op) = &mut self.active else { return };
        let Geom::Text { geom, style, .. } = &mut op.geom else { return };
        let ratio = size_pt / style.size_pt;
        *geom = geom.resized(geom.rect.width() * ratio, geom.rect.height() * ratio, 0.0);
        style.size_pt = size_pt;
        op.paint.opacity = opacity;
        let d = op.update();
        self.preview_dirty.add(d);
    }

    /// Hands the text object picked up by the Pointer over to the Text tool.
    fn edit_lifted_text(&mut self, p: Point) {
        match std::mem::take(&mut self.active) {
            Active::Shape(ShapeOp { geom: Geom::Text { geom, text, style, auto_size }, paint, lifted, .. }) => {
                self.open_text_object(geom, &text, paint.text_style(&style), auto_size, lifted, p);
            }
            other => self.active = other,
        }
    }

    /// Takes object `index` of layer `layer` off the document to edit it, starting a move at `p`.
    fn lift_object(&mut self, layer: usize, index: usize, p: Point) {
        let Some(lifted) = self.take_object_for_edit(layer, index) else { return };
        let shape = lifted.original.clone();
        let mut op = ShapeOp {
            geom: Geom::from_shape(&shape.geom),
            paint: shape.paint,
            applied: self.shape_paint(false),
            swap: false,
            drag: None,
            raster: shape.raster().cloned(),
            stale: false,
            vector: true,
            lifted: None,
            canvas: (self.doc.width(), self.doc.height()),
            last_click: Some((Instant::now(), p)),
            drag_button: PointerButton::Primary,
            selected_point: None,
        };
        op.drag = Some(op.start_move(p));
        op.lifted = Some(lifted);
        self.preview_dirty.add(op.bounds());
        self.active = Active::Shape(op);
    }

    /// Re-renders the shape being edited if a drag changed it. Called once per frame, so a burst
    /// of pointer moves costs a single rasterization.
    pub fn flush_shape(&mut self) {
        if let Active::Shape(op) = &mut self.active
            && op.stale
        {
            let d = op.update();
            self.preview_dirty.add(d);
        }
    }

    pub fn shape_pointer_move(&mut self, op: &mut ShapeOp, p: Point, m: Modifiers) {
        let Some(drag) = op.drag.clone() else {
            // While an arrow is drawn, its tip follows the pointer without a button held.
            if let Geom::Arrow { points, drawing: true } = &mut op.geom
                && let [.., prev, tip] = points.as_mut_slice()
            {
                *tip = if m.shift { snap_45(*prev, p) } else { p };
                op.stale = true;
            }
            return;
        };
        if let Drag::Move { start, orig, at } = &drag {
            // Whole pixels, so the existing raster can just be shifted instead of redrawn.
            let (dx, dy) = ((p.x - start.x).round(), (p.y - start.y).round());
            op.geom = orig.translated(dx, dy);
            match (at, &mut op.raster) {
                (Some((x, y)), Some(r)) if !op.stale => {
                    let before = r.bounds();
                    r.x = x + dx as i32;
                    r.y = y + dy as i32;
                    self.preview_dirty.add(before.union(&r.bounds()));
                }
                _ => op.stale = true,
            }
            return;
        }
        // Sizing a text box by hand ends its automatic sizing.
        if let (Geom::Text { auto_size, .. }, Drag::Box(BoxHit::Handle(_), ..)) = (&mut op.geom, &drag) {
            *auto_size = false;
        }
        match (&mut op.geom, drag) {
            (Geom::Box { geom, .. }, Drag::Create { start }) => {
                let end = if m.shift { square_from(start, p) } else { p };
                geom.rect = RectF::from_points(start, end);
            }
            (Geom::Line { b, .. }, Drag::Create { start }) => *b = if m.shift { snap_45(start, p) } else { p },
            (Geom::Curve { b, c1, c2, .. }, Drag::Create { start }) => {
                *b = if m.shift { snap_45(start, p) } else { p };
                *c1 = start.lerp(*b, 1.0 / 3.0);
                *c2 = start.lerp(*b, 2.0 / 3.0);
            }
            (Geom::Arrow { points, .. }, Drag::Create { .. }) => {
                if let [.., prev, last] = points.as_mut_slice() {
                    *last = if m.shift { snap_45(*prev, p) } else { p };
                }
            }
            (Geom::Arrow { points, .. }, Drag::ArrowPoint(i)) => {
                let prev = i.checked_sub(1).and_then(|j| points.get(j).copied());
                if let Some(q) = points.get_mut(i) {
                    *q = match prev {
                        Some(prev) if m.shift => snap_45(prev, p),
                        _ => p,
                    };
                }
            }
            (Geom::PolygonDraft { points }, Drag::Create { .. } | Drag::PolyPoint) => {
                if let [.., prev, last] = points.as_mut_slice() {
                    *last = if m.shift { snap_45(*prev, p) } else { p };
                }
            }
            (Geom::Box { geom, .. } | Geom::Text { geom, .. }, Drag::Box(hit, start, orig)) => *geom = orig.drag(hit, start, p, m.shift),
            (Geom::Line { a, b }, Drag::Endpoint(end)) => {
                if end {
                    *b = if m.shift { snap_45(*a, p) } else { p };
                } else {
                    *a = if m.shift { snap_45(*b, p) } else { p };
                }
            }
            (Geom::Curve { a, b, c1, c2, .. }, Drag::Bend(stage)) => {
                // Pull the curve so it passes near the pointer (Paint's bend behaviour).
                let target = Point::new(2.0 * p.x - (a.x + b.x) / 2.0, 2.0 * p.y - (a.y + b.y) / 2.0);
                if stage == 1 {
                    *c1 = target;
                    *c2 = target;
                } else {
                    *c2 = target;
                }
            }
            _ => {}
        }
        op.stale = true;
    }

    /// Ends a drag; returns the shape if it is still being edited.
    pub fn shape_pointer_up(&mut self, mut op: ShapeOp, _p: Point, button: PointerButton) -> Option<ShapeOp> {
        // Releasing another button (a right click that ended an arrow mid-drag) does not end it.
        if op.drag.is_some() && button != op.drag_button {
            return Some(op);
        }
        let Some(drag) = op.drag.take() else { return Some(op) };
        // Shapes drawn in one drag are formed when the button is released (the curve, the
        // curved arrow and the polygon go on with more clicks).
        let formed = matches!(drag, Drag::Create { .. }) && matches!(op.geom, Geom::Box { .. } | Geom::Line { .. });
        match (&mut op.geom, drag) {
            // A click without a drag: the tip keeps following the pointer.
            (Geom::Arrow { points, .. }, Drag::Create { .. }) if points.len() == 2 && points[0].distance(points[1]) < 1.0 => {}
            // The dragged or clicked tip is fixed; a new one follows the pointer from there.
            (Geom::Arrow { points, drawing: true }, Drag::Create { .. } | Drag::ArrowPoint(_)) => {
                if let Some(&tip) = points.last() {
                    points.push(tip);
                }
            }
            (Geom::Curve { stage, .. }, Drag::Create { .. }) => *stage = 0,
            (Geom::Curve { stage, .. }, Drag::Bend(s)) => {
                *stage = s;
                // A curve is committed after its second bend (and, in vector mode, hands over to
                // the Pointer).
                if s == 2 {
                    self.commit_shape(op);
                    if self.doc.vector_mode() {
                        self.tools.vector_pointer = true;
                    }
                    return None;
                }
            }
            (Geom::PolygonDraft { .. }, _) => {
                // Keep collecting points; the next click adds a vertex.
                let n = match &op.geom {
                    Geom::PolygonDraft { points } => points.len(),
                    _ => 0,
                };
                if n < 2 {
                    return None;
                }
            }
            (Geom::Box { geom, .. }, Drag::Create { .. }) if geom.rect.width() < 1.0 && geom.rect.height() < 1.0 => {
                // A click without a drag draws nothing.
                self.preview_dirty.add(op.bounds());
                return None;
            }
            (Geom::Line { a, b }, Drag::Create { .. }) if a.distance(*b) < 1.0 => {
                self.preview_dirty.add(op.bounds());
                return None;
            }
            _ => {}
        }
        // In vector mode a formed shape hands over to the Pointer; it stays editable meanwhile.
        if formed && self.doc.vector_mode() {
            self.tools.vector_pointer = true;
        }
        Some(op)
    }

    pub fn commit_shape(&mut self, mut op: ShapeOp) {
        // Finishing can still change the shape (a polygon closes, an arrow drops the leg that
        // followed the pointer): the area it covered before is redrawn too, along with the new one.
        self.preview_dirty.add(op.bounds());
        if let Geom::PolygonDraft { points } = &op.geom
            && points.len() >= 2
        {
            finish_polygon(&mut op);
            op.update();
        }
        if matches!(op.geom, Geom::Arrow { drawing: true, .. }) && !finish_arrow(&mut op) {
            self.cancel_shape(op);
            return;
        }
        if op.stale {
            op.update();
        }
        self.preview_dirty.add(op.bounds());
        if op.vector {
            self.commit_vector(op);
            return;
        }
        let Some(r) = op.raster.take() else { return };
        let bounds = r.bounds();
        let change = self.doc.edit_active(bounds, |layer| raster::draw_over(layer, &r.pixmap, r.x, r.y, 1.0));
        self.history.push_one("shape", change);
    }

    /// Puts the shape on its layer as a vector object (back in its place when it was picked up).
    fn commit_vector(&mut self, mut op: ShapeOp) {
        let Some(geom) = op.geom.to_shape() else {
            self.cancel_shape(op);
            return;
        };
        let shape = VectorShape::with_raster(geom, op.paint, op.raster.take());
        self.place_object(shape, op.lifted.take(), "shape");
    }

    /// Puts a finished object on its layer, back in its place when it was picked up. `label`
    /// names the undo step of a new object.
    pub(super) fn place_object(&mut self, shape: VectorShape, lifted: Option<Lifted>, label: &'static str) {
        // A shape moved (or drawn) entirely off the canvas shows nowhere: it is not kept.
        let visible = !shape.bounds().intersect(&self.doc.bounds()).is_empty();
        match lifted {
            Some(l) if !visible => self.history.push("delete shape", vec![Change::document(l.before)]),
            Some(l) => {
                let changed = !shape.same_as(&l.original);
                self.doc.insert_object(l.layer, l.index, shape);
                if changed {
                    self.history.push("edit shape", vec![Change::document(l.before)]);
                }
            }
            None if !visible => {}
            None => {
                let before = self.doc.snapshot();
                self.doc.add_object(shape);
                self.history.push(label, vec![Change::document(before)]);
            }
        }
    }

    /// Drops the shape being edited; a picked-up vector object goes back unchanged.
    pub fn cancel_shape(&mut self, op: ShapeOp) {
        self.preview_dirty.add(op.bounds());
        if let Some(l) = op.lifted {
            self.doc.insert_object(l.layer, l.index, l.original);
        }
    }

    /// Deletes the shape being edited (with an undo step when it was a picked-up object).
    pub fn delete_shape(&mut self, op: ShapeOp) {
        self.preview_dirty.add(op.bounds());
        if let Some(l) = op.lifted {
            self.history.push("delete shape", vec![Change::document(l.before)]);
        }
    }

    /// Applies new outline/fill/size/colors to the shape being edited.
    pub fn restyle_shape(&mut self) {
        let swap = match &self.active {
            Active::Shape(op) => op.swap,
            _ => return,
        };
        let new = self.shape_paint(swap);
        if let Active::Shape(op) = &mut self.active {
            let (old, p) = (op.applied, &mut op.paint);
            if new.outline != old.outline {
                p.outline = new.outline;
            }
            if new.fill != old.fill {
                p.fill = new.fill;
            }
            if new.width != old.width {
                p.width = new.width;
            }
            if new.outline_color != old.outline_color {
                p.outline_color = new.outline_color;
            }
            if new.fill_color != old.fill_color {
                p.fill_color = new.fill_color;
            }
            if new.opacity != old.opacity {
                p.opacity = new.opacity;
            }
            op.applied = new;
            let d = op.update();
            self.preview_dirty.add(d);
        }
    }

    pub fn shape_cursor(&self, pos: Pos2) -> Option<egui::CursorIcon> {
        use egui::CursorIcon as C;
        let p = self.view.to_canvas(pos);
        let tol = 6.0 / self.view.scale();
        // A shape can be grabbed (open hand) and is held while it moves (closed hand).
        let grab = |hit: BoxHit, angle: f32| if hit == BoxHit::Inside { C::Grab } else { hit.cursor(angle) };
        let on_op = match &self.active {
            Active::Shape(op) => match (&op.drag, &op.geom) {
                (Some(Drag::Move { .. }), _) => Some(C::Grabbing),
                (Some(Drag::Box(hit, _, orig)), _) => Some(hit.cursor(orig.angle)),
                (Some(Drag::Endpoint(_) | Drag::ArrowPoint(..)), _) => Some(C::Crosshair),
                (Some(_), _) => None,
                (None, Geom::Box { geom, .. } | Geom::Text { geom, .. }) => {
                    geom.hit(p, tol, 24.0 / self.view.scale()).map(|h| grab(h, geom.angle))
                }
                (None, Geom::Line { a, b }) => {
                    if a.distance(p) <= tol * 1.5 || b.distance(p) <= tol * 1.5 {
                        Some(C::Crosshair)
                    } else if near_segment(*a, *b, p, tol + op.paint.width) {
                        Some(C::Grab)
                    } else {
                        None
                    }
                }
                (None, Geom::Curve { stage: 2, .. }) => op.raster.as_ref().is_some_and(|r| r.hit(p, tol)).then_some(C::Grab),
                (None, Geom::Arrow { drawing: true, .. }) => Some(C::Crosshair),
                (None, Geom::Arrow { points, .. }) => {
                    if points.iter().any(|q| q.distance(p) <= tol * 1.5) {
                        Some(C::Crosshair)
                    } else if op.raster.as_ref().is_some_and(|r| r.hit(p, tol)) {
                        Some(C::Grab)
                    } else {
                        None
                    }
                }
                _ => None,
            },
            _ => None,
        };
        // The Pointer keeps the normal arrow over objects; handles still show what they do, and
        // a move in progress the closed hand.
        if self.vector_pointer() {
            return Some(on_op.filter(|c| *c != C::Grab).unwrap_or(C::Default));
        }
        // In vector mode, objects can be grabbed.
        on_op.or_else(|| (self.doc.vector_mode() && self.doc.object_at(p, tol).is_some()).then_some(C::Grab))
    }
}

fn segment_distance(a: Point, b: Point, p: Point) -> f32 {
    let (vx, vy) = (b.x - a.x, b.y - a.y);
    let len2 = vx * vx + vy * vy;
    let t = if len2 == 0.0 { 0.0 } else { (((p.x - a.x) * vx + (p.y - a.y) * vy) / len2).clamp(0.0, 1.0) };
    Point::new(a.x + vx * t, a.y + vy * t).distance(p)
}

fn near_segment(a: Point, b: Point, p: Point, tol: f32) -> bool {
    segment_distance(a, b, p) <= tol
}

/// Ends the drawing of an arrow. The leg following the pointer with no button held is dropped
/// (the arrow ends at the last point clicked); a leg being dragged is kept. Returns false when
/// nothing is left (no leg was fixed yet).
fn finish_arrow(op: &mut ShapeOp) -> bool {
    let following = op.drag.is_none();
    let Geom::Arrow { points, drawing } = &mut op.geom else { return true };
    *drawing = false;
    if following {
        points.pop();
    }
    op.drag = None;
    op.stale = true;
    points.len() >= 2 && points[0].distance(points[points.len() - 1]) >= 1.0
}

/// Turns a clicked polygon into an editable box shape with normalized points.
fn finish_polygon(op: &mut ShapeOp) {
    let Geom::PolygonDraft { points } = &op.geom else { return };
    if points.len() < 2 {
        return;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in points {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    let rect = RectF::from_points(Point::new(x0, y0), Point::new(x1.max(x0 + 1.0), y1.max(y0 + 1.0)));
    let norm = points.iter().map(|p| ((p.x - rect.min.x) / rect.width(), (p.y - rect.min.y) / rect.height())).collect();
    op.geom = Geom::Box { kind: ShapeKind::Polygon, geom: BoxGeom { rect, angle: 0.0 }, points: Some(norm) };
    op.drag = None;
}

/// Draws the editing handles of the shape.
pub fn paint_overlay(op: &ShapeOp, view: &View, painter: &egui::Painter, t: &Theme) {
    match &op.geom {
        Geom::Box { geom, .. } if op.editing() => geom.paint(view, painter, t, true),
        // A text box only shows through its frame, also while it is dragged.
        Geom::Text { geom, .. } => geom.paint(view, painter, t, true),
        Geom::Line { a, b } if op.editing() => paint_point_handles(view, painter, &[*a, *b]),
        // Every point of an arrow has its own handle.
        Geom::Arrow { points, .. } if op.editing() => {
            paint_point_handles(view, painter, points);
            // The point marked for removal is filled with the accent color.
            if let Some(p) = op.selected_point.and_then(|i| points.get(i)) {
                painter.rect(
                    widgets::square(view.to_screen(*p), 7.0),
                    0,
                    t.accent,
                    egui::Stroke::new(1.0, egui::Color32::from_gray(60)),
                    egui::StrokeKind::Inside,
                );
            }
        }
        Geom::Curve { a, b, .. } => {
            for p in [a, b] {
                painter.circle_stroke(view.to_screen(*p), 3.5, egui::Stroke::new(1.0, t.accent));
            }
        }
        _ => {}
    }
}

fn paint_point_handles(view: &View, painter: &egui::Painter, points: &[Point]) {
    for p in points {
        painter.rect(
            widgets::square(view.to_screen(*p), 7.0),
            0,
            egui::Color32::WHITE,
            egui::Stroke::new(1.0, egui::Color32::from_gray(60)),
            egui::StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bgpaint_core::color::Color;
    use bgpaint_core::shapes::ShapeStyle;

    #[test]
    fn snapping() {
        let s = snap_45(Point::new(0.0, 0.0), Point::new(10.0, 1.0));
        assert!((s.y).abs() < 1e-3 && (s.x - 10.05).abs() < 0.1);
        let q = square_from(Point::new(0.0, 0.0), Point::new(-4.0, 9.0));
        assert_eq!((q.x, q.y), (-9.0, 9.0));
        assert!(near_segment(Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(5.0, 2.0), 3.0));
        assert!(!near_segment(Point::new(0.0, 0.0), Point::new(10.0, 0.0), Point::new(15.0, 0.0), 3.0));
    }

    #[test]
    fn geometry_round_trips_through_vector_objects() {
        let shapes = [
            Geom::Box {
                kind: ShapeKind::Polygon,
                geom: BoxGeom { rect: RectF::from_xywh(1.0, 2.0, 30.0, 40.0), angle: 15.0 },
                points: Some(vec![(0.0, 0.0), (1.0, 0.5), (0.2, 1.0)]),
            },
            Geom::Line { a: Point::new(1.0, 1.0), b: Point::new(9.0, 4.0) },
            Geom::Curve { a: Point::new(0.0, 0.0), b: Point::new(9.0, 0.0), c1: Point::new(2.0, 5.0), c2: Point::new(7.0, 5.0), stage: 2 },
            Geom::Arrow { points: vec![Point::new(0.0, 0.0), Point::new(5.0, 9.0), Point::new(12.0, 3.0)], drawing: false },
        ];
        for g in shapes {
            let back = Geom::from_shape(&g.to_shape().unwrap());
            assert_eq!(back.to_shape(), g.to_shape());
            // Moving the editable geometry moves the object the same way.
            assert_eq!(g.translated(3.0, -2.0).to_shape(), g.to_shape().map(|s| s.translated(3.0, -2.0)));
        }
        assert!(Geom::PolygonDraft { points: vec![Point::new(0.0, 0.0)] }.to_shape().is_none());
        let Geom::PolygonDraft { points } = Geom::PolygonDraft { points: vec![Point::new(1.0, 1.0)] }.translated(1.0, 2.0) else {
            panic!()
        };
        assert_eq!(points, vec![Point::new(2.0, 3.0)]);
    }

    #[test]
    fn resizing_a_text_box_keeps_its_anchor_in_place() {
        for angle in [0.0, 30.0, 200.0] {
            let g = BoxGeom { rect: RectF::from_xywh(10.0, 20.0, 100.0, 40.0), angle };
            let s = g.resized(250.0, 100.0, 0.0);
            assert!((s.rect.width() - 250.0).abs() < 0.01 && (s.rect.height() - 100.0).abs() < 0.01);
            assert_eq!(s.angle, angle);
            let (a, b) = (g.to_world(g.rect.min), s.to_world(s.rect.min));
            assert!(a.distance(b) < 0.01, "{angle}: {a:?} -> {b:?}");
            // A right-aligned box keeps its top right corner.
            let s = g.resized(40.0, 100.0, 1.0);
            let (a, b) = (g.to_world(Point::new(g.rect.max.x, g.rect.min.y)), s.to_world(Point::new(s.rect.max.x, s.rect.min.y)));
            assert!(a.distance(b) < 0.51, "{angle}: {a:?} -> {b:?}");
        }
    }

    #[test]
    fn delete_removes_only_the_marked_arrow_point() {
        let paint = ShapePaint {
            outline: ShapeStyle::Solid,
            fill: ShapeStyle::None,
            width: 2.0,
            outline_color: Color::BLACK,
            fill_color: Color::WHITE,
            opacity: 1.0,
        };
        let pts = vec![Point::new(0.0, 0.0), Point::new(30.0, 40.0), Point::new(60.0, 0.0), Point::new(90.0, 40.0)];
        let mut op = ShapeOp {
            geom: Geom::Arrow { points: pts.clone(), drawing: false },
            paint,
            applied: paint,
            swap: false,
            drag: None,
            raster: None,
            stale: false,
            vector: false,
            lifted: None,
            canvas: (100, 100),
            last_click: None,
            drag_button: PointerButton::Primary,
            selected_point: None,
        };
        // Nothing marked: Delete is for the whole shape.
        assert!(!op.delete_selected_point());
        op.selected_point = Some(1);
        assert!(op.delete_selected_point());
        let Geom::Arrow { points, .. } = &op.geom else { panic!() };
        assert_eq!(points, &vec![pts[0], pts[2], pts[3]]);
        assert_eq!(op.selected_point, None);
        // The tip and the start can go too: the head moves to the point before.
        op.selected_point = Some(2);
        op.delete_selected_point();
        let Geom::Arrow { points, .. } = &op.geom else { panic!() };
        assert_eq!(points, &vec![pts[0], pts[2]]);
        // Down to two points, nothing more is removed.
        op.selected_point = Some(0);
        assert!(op.delete_selected_point());
        let Geom::Arrow { points, .. } = &op.geom else { panic!() };
        assert_eq!(points.len(), 2);
    }

    #[test]
    fn ending_an_arrow_drops_the_leg_following_the_pointer() {
        let paint = ShapePaint {
            outline: ShapeStyle::Solid,
            fill: ShapeStyle::None,
            width: 2.0,
            outline_color: Color::BLACK,
            fill_color: Color::WHITE,
            opacity: 1.0,
        };
        let arrow = |points: Vec<Point>, drag: Option<Drag>| ShapeOp {
            geom: Geom::Arrow { points, drawing: true },
            paint,
            applied: paint,
            swap: false,
            drag,
            raster: None,
            stale: false,
            vector: false,
            lifted: None,
            canvas: (100, 100),
            last_click: None,
            drag_button: PointerButton::Primary,
            selected_point: None,
        };
        let (a, b, c) = (Point::new(0.0, 0.0), Point::new(40.0, 0.0), Point::new(40.0, 30.0));
        let points = |op: &ShapeOp| match &op.geom {
            Geom::Arrow { points, drawing } => {
                assert!(!drawing);
                points.clone()
            }
            _ => panic!(),
        };
        // The tip following the pointer is dropped: the arrow ends at the last click.
        let mut op = arrow(vec![a, b, c], None);
        assert!(finish_arrow(&mut op));
        assert_eq!(points(&op), vec![a, b]);
        // A tip being dragged stays.
        let mut op = arrow(vec![a, b, c], Some(Drag::ArrowPoint(2)));
        assert!(finish_arrow(&mut op));
        assert_eq!(points(&op), vec![a, b, c]);
        // Only the first click: nothing is left.
        let mut op = arrow(vec![a, c], None);
        assert!(!finish_arrow(&mut op));
    }

    #[test]
    fn polygon_finishes_into_normalized_box() {
        let mut op = ShapeOp {
            geom: Geom::PolygonDraft { points: vec![Point::new(10.0, 10.0), Point::new(30.0, 10.0), Point::new(20.0, 40.0)] },
            paint: ShapePaint {
                outline: ShapeStyle::Solid,
                fill: ShapeStyle::None,
                width: 1.0,
                outline_color: Color::BLACK,
                fill_color: Color::WHITE,
                opacity: 1.0,
            },
            applied: ShapePaint {
                outline: ShapeStyle::Solid,
                fill: ShapeStyle::None,
                width: 1.0,
                outline_color: Color::BLACK,
                fill_color: Color::WHITE,
                opacity: 1.0,
            },
            swap: false,
            drag: None,
            raster: None,
            stale: false,
            vector: false,
            lifted: None,
            canvas: (100, 100),
            last_click: None,
            drag_button: PointerButton::Primary,
            selected_point: None,
        };
        finish_polygon(&mut op);
        let Geom::Box { geom, points: Some(pts), .. } = &op.geom else { panic!() };
        assert_eq!(geom.rect, RectF::from_xywh(10.0, 10.0, 20.0, 30.0));
        assert_eq!(pts[2], (0.5, 1.0));
        assert!(!op.update().is_empty());
    }
}
