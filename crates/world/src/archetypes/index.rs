//! Archtype indexes.

/// Identity of a game object. Used to look up game objects (`Thing`s) within a
/// `World`.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct PlayerIndex(u32);

impl From<u32> for PlayerIndex {
    fn from(value: u32) -> Self {
        Self(value)
    }
}
impl From<usize> for PlayerIndex {
    fn from(value: usize) -> Self {
        Self(value as u32)
    }
}
impl From<PlayerIndex> for usize {
    fn from(value: PlayerIndex) -> Self {
        value.0 as usize
    }
}

/// Index to address a graphic.
#[derive(Debug, Copy, Clone, Hash, PartialEq, Eq)]
pub struct GfxIndex(pub(crate) u32);
impl From<u16> for GfxIndex {
    fn from(value: u16) -> Self {
        Self(value as u32)
    }
}

impl From<GfxIndex> for u16 {
    fn from(value: GfxIndex) -> Self {
        value.0 as u16
    }
}

impl From<u32> for GfxIndex {
    fn from(value: u32) -> Self {
        Self(value)
    }
}

impl From<usize> for GfxIndex {
    fn from(value: usize) -> Self {
        Self(value as u32)
    }
}
