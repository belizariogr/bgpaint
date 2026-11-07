//! Which color the "Edit colors" dialog writes to.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ColorTarget {
    /// The selected palette slot (Color 1 or Color 2).
    Active,
    /// The document background.
    Background,
}
