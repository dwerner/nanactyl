use glam::{Mat4, Vec3};
use hecs::{Bundle, Entity};

use crate::graphics::GfxIndex;

#[derive(Debug, Default)]
pub struct Camera {
    pub fov: f32,
    pub near: f32,
    pub far: f32,
}

#[derive(Debug, Default)]
pub struct Position {
    pub position: Vec3,
    pub rotation: Mat4,
}

#[derive(Debug, Default)]
pub struct Graphics {
    pub gfx: GfxIndex,
    pub scale: f32,
}

#[derive(Debug, Default)]
pub struct Physics {
    pub velocity: Vec3,
    pub acceleration: Vec3,
    pub mass: f32,
}

#[derive(Debug, Default)]
pub struct TransformNode {
    /// Potential parent.
    pub parent: Option<Entity>,
    /// Children.
    pub children: Vec<Entity>,
    /// Position in world space.
    pub position: Vec3,
    /// Model matrix.
    pub model: Mat4,
    /// World Matrix.
    pub world: Mat4,
}

#[derive(Default, Bundle)]
pub struct Player {
    pub camera: Camera,
    pub position: Position,
    pub graphics: Graphics,
    pub physics: Physics,
    pub transform_node: TransformNode,
}

#[derive(Debug, Default)]
pub struct AudioSource {
    enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn playing_with_hecs() {
        let mut world = hecs::World::new();

        let mut player = Player::default();
        player.camera.far = 42.0;
        let player_id = world.spawn(player);
        let mut root = TransformNode::default();
        root.children.push(player_id);

        let entity = {
            let mut query = world.query::<(&Camera, &Position)>();
            let (entity, (camera, pos)) = query.iter().next().unwrap();
            println!("{:?}", entity);
            assert_eq!(camera.far, 42.0);

            let mut nodes = world.query::<&TransformNode>();
            for node in nodes.iter() {
                println!("{:#?}", node);
            }
            entity
        };

        // add a single component
        world.insert_one(entity, AudioSource::default()).unwrap();
    }
}
