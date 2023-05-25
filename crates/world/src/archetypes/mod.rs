use std::sync::atomic::AtomicU32;

use self::object::ObjectArchetype;
use self::player::PlayerArchetype;
use self::views::{CameraRef, DrawRef, PhysicsRef};

pub mod dynamic;
pub mod macros;
pub mod object;
pub mod player;
pub mod views;
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

/// Entity storage for the game. This is a collection of archetypes, for which
/// several ref types might be implemented for systems to iterate over the data
/// of.
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct EntityArchetypes {
    players: PlayerArchetype,
    objects: ObjectArchetype,
}

impl EntityArchetypes {
    /// Spawn a player into the archetype, parameterized by the builder.
    pub fn spawn_player(
        &mut self,
        entity: u32,
        builder: player::PlayerBuilder,
    ) -> Result<(), player::PlayerError> {
        self.players.spawn(entity, builder)
    }

    /// Spawn an object into the archetype, parameterized by the builder.
    pub fn spawn_object(
        &mut self,
        entity: u32,
        builder: object::ObjectBuilder,
    ) -> Result<(), object::ObjectError> {
        self.objects.spawn(entity, builder)
    }

    /// Iterate over all archetypes and view of their physics.
    pub fn physics_iter_mut(&mut self) -> impl Iterator<Item = PhysicsRef<'_>> {
        self.players
            .physics_iter_mut()
            .chain(self.objects.physics_iter_mut())
    }

    /// Iterate over all archetypes and view of their drawables.
    pub fn draw_iter_mut(&mut self) -> impl Iterator<Item = DrawRef<'_>> {
        self.players
            .draw_iter_mut()
            .chain(self.objects.draw_iter_mut())
    }

    /// Iterate over all cameras in the archetype.
    pub fn camera_iter_mut(&mut self) -> impl Iterator<Item = CameraRef<'_>> {
        self.players.camera_iter_mut()
    }

    /// Iterate over all players in the archetype.
    pub fn players_iter_mut(&mut self) -> impl Iterator<Item = player::PlayerRef<'_>> {
        self.players.iter_mut()
    }

    /// Iterate over all objects in the archetype.
    pub fn objects_iter_mut(&mut self) -> impl Iterator<Item = object::ObjectRef<'_>> {
        self.objects.iter_mut()
    }

    /// Get a player ref.
    pub fn get_player_mut(&mut self, entity: u32) -> Option<player::PlayerRef<'_>> {
        self.players.get_mut(entity)
    }

    /// Get an object ref.
    pub fn get_object_mut(&mut self, entity: u32) -> Option<object::ObjectRef<'_>> {
        self.objects.get_mut(entity)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[smol_potat::test]
    async fn par_iter_test() {
        let mut executor = core_executor::ThreadPoolExecutor::new(8);
        let mut archetypes = EntityArchetypes::default();

        for i in 0..100 {
            archetypes
                .spawn_player(i, player::PlayerBuilder::default())
                .unwrap();
            for i in 0..10 {
                archetypes
                    .spawn_object(i, object::ObjectBuilder::default())
                    .unwrap();
            }
        }

        // This won't compile due to thread pool lifetimes. Work is on-going
        // there.

        // let player_iter = archetypes.players_iter_mut();
        // let object_iter = archetypes.objects_iter_mut();
        // let player_updates = executor.spawn_on_core(0, async {
        //     for player in player_iter {
        //         println!("Player: {:?}", player);
        //     }
        // });
        // let object_updates = executor.spawn_on_core(1, async {
        //     for object in object_iter {
        //         println!("Object: {:?}", object);
        //     }
        // });
        // futures_util::future::join(player_updates, object_updates).await;
    }
}
