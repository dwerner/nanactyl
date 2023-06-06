use std::f32::consts::PI;
use std::time::Duration;

use gfx::Model;
use logger::{info, LogLevel, Logger};
use world::bundles::{Player, StaticObject};
use world::components::spatial::SpatialHierarchyNode;
use world::components::WorldTransform;
use world::{AssetLoaderStateAndWorldLock, Vec3};

pub struct AssetLoaderPlugin {
    logger: Logger,
}

impl AssetLoaderPlugin {
    pub fn new() -> Self {
        Self {
            logger: LogLevel::Info.logger().sub("asset-loader"),
        }
    }

    pub fn load(&mut self, state: &mut AssetLoaderStateAndWorldLock) {
        let logger = &state.world.logger.sub("asset-loader");
        logger.maybe_set_filter(state.world.logger.get_filter());

        // This plugin 'owns' the root entity and all it's children's lifetimes.
        let _ = std::mem::replace(&mut state.world.hecs_world, Default::default());
        state.world.root = Some(state.world.hecs_world.spawn((WorldTransform::default(),)));

        let world = &mut state.world;
        let root = world.root.unwrap();
        info!(self.logger.sub("load"), "asset loader plugin loaded.");

        let tank_model = Model::load_obj(
            "assets/models/static/tank.obj",
            "assets/shaders/spv/default_vertex.spv",
            "assets/shaders/spv/default_fragment.spv",
        )
        .unwrap();
        let tank_gfx = world.add_model(tank_model);

        let cube_model = Model::load_obj(
            "assets/models/static/cube.obj",
            "assets/shaders/spv/default_vertex.spv",
            "assets/shaders/spv/default_fragment.spv",
        )
        .unwrap();
        let cube_gfx = world.add_model(cube_model);

        let flip_angles = Vec3::new(0.0, 0.0 * PI, 1.0 * PI);

        for (x, z) in [(10.0, 10.0), (-10.0, -10.0)].into_iter() {
            info!(logger, "adding player camera object at: {}, {}", x, z);
            let pos = Vec3::new(x, 0.0, z);
            let tank = Player::new(
                tank_gfx,
                SpatialHierarchyNode::new_at(root, pos).with_angles(flip_angles),
            );
            let _tank_id = world.add_player(tank);
        }

        // initialize some state, lots of model_object entities
        for i in -4..4i32 {
            for j in -4..4i32 {
                let model_prefab = if (i + j) % 2 == 0 { tank_gfx } else { cube_gfx };
                let (x, z) = (i as f32, j as f32);
                let object = StaticObject::new(
                    model_prefab,
                    SpatialHierarchyNode::new_at(root, Vec3::new(x * 4.0, 2.0, z * 10.0))
                        .with_angles(flip_angles),
                );

                // TODO: add_object
                world.hecs_world.spawn(object);
            }
        }

        let sky_model = Model::load_obj(
            "assets/models/static/skybox.obj",
            "assets/shaders/spv/skybox_vertex.spv",
            "assets/shaders/spv/skybox_fragment.spv",
        )
        .unwrap();

        let sky_prefab = world.add_model(sky_model);
        let sky = StaticObject::new(
            sky_prefab,
            SpatialHierarchyNode::new_with_scale(root, 200.0).with_angles(flip_angles),
        );
        world.hecs_world.spawn(sky);
    }

    pub fn update(&mut self, state: &mut AssetLoaderStateAndWorldLock, delta_time: &Duration) {}

    pub fn unload(&mut self, state: &mut AssetLoaderStateAndWorldLock) {
        let log = self.logger.sub("unload");
        state.world.players.clear();
        let _ = std::mem::replace(&mut state.world.hecs_world, Default::default());
        state.world.root.take();
        info!(
            log,
            "unloaded asset loader plugin ({})", state.world.stats.updates
        );
    }
}
