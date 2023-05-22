use std::sync::atomic::AtomicU32;

pub mod macros;
pub mod player;

// TODO pub mod net;

const NEXT_ENTITY: AtomicU32 = AtomicU32::new(0);

pub fn next_entity() -> u32 {
    NEXT_ENTITY.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
}

/// Every archetype needs the same basic interface:
pub trait Archetype {
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

    /// Iterate over all items in the archetype
    fn iter_mut(&mut self) -> Self::IterMut<'_>;

    /// Get a single mutable reference to an item in the archetype.
    fn get_mut(&mut self, entity: u32) -> Option<Self::ItemRef<'_>>;

    /// Spawn an entity into the archetype, parameterized by the builder.
    fn spawn(&mut self, entity: u32, builder: Self::Builder) -> Result<(), Self::Error>;

    fn despawn(&mut self, entity: u32) -> Result<(), Self::Error>;

    /// Return a builder for creating entitites within this archetype.
    fn builder(&self) -> Self::Builder;

    /// Set the default builder for this archetype. This is useful if you want
    /// to set some fields once, and merely copy them to new entities, changing
    /// only the fields that you care about.
    fn set_default_builder(&mut self, builder: Self::Builder);
}
