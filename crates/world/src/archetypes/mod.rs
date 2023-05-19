pub mod graphics;
pub mod index;
pub mod macros;
pub mod player;

// TODO pub mod net;

/// Every archetype needs the same basic interface:
pub trait Archetype {
    /// The index type for this archetype.
    type Index: From<usize>;

    /// A reference to an item in the archetype.
    type ItemRef<'a>: 'a
    where
        Self: 'a;

    /// An iterator over all items in the archetype.
    type IterMut<'a>: Iterator<Item = Self::ItemRef<'a>> + 'a
    where
        Self: 'a;

    /// A builder for creating entities within this archetype.
    type Builder;

    /// The error type returned by this archetype.
    type Error;

    /// Return the last index of the archetype.
    fn len(&self) -> Self::Index;

    /// Iterate over all items in the archetype
    fn iter_mut(&mut self) -> Self::IterMut<'_>;

    /// Get a single mutable reference to an item in the archetype.
    fn get_mut(&mut self, index: Self::Index) -> Option<Self::ItemRef<'_>>;

    /// Spawn an entity into the archetype, parameterized by the builder.
    fn spawn(&mut self, builder: Self::Builder) -> Result<Self::Index, Self::Error>;

    fn despawn(&mut self, index: Self::Index) -> Result<(), Self::Error>;

    /// Return a builder for creating entitites within this archetype.
    fn builder(&self) -> Self::Builder;

    /// Set the default builder for this archetype. This is useful if you want
    /// to set some fields once, and merely copy them to new entities, changing
    /// only the fields that you care about.
    fn set_default_builder(&mut self, builder: Self::Builder);
}
