pub mod graphics;
pub mod index;
pub mod player;

// TODO pub mod net;

/// Every archetype needs the same basic interface:
pub trait Archetype {
    type Index: From<usize>;

    type ItemRef<'a>: 'a
    where
        Self: 'a;

    type IterMut<'a>: Iterator<Item = Self::ItemRef<'a>> + 'a
    where
        Self: 'a;

    type Builder;
    type Error;

    fn len(&self) -> Self::Index;

    fn iter_mut(&mut self) -> Self::IterMut<'_>;

    fn get_mut(&mut self, index: Self::Index) -> Option<Self::ItemRef<'_>>;

    fn spawn(&mut self, builder: Self::Builder) -> Result<Self::Index, Self::Error>;

    fn despawn(&mut self, index: Self::Index) -> Result<(), Self::Error>;
}
