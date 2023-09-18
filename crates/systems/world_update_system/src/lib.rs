//! Plugin: `world_update_system`
//! Implements a plugin (see crates/plugin-loader) for prototyping 'world
//! updates'. This means anything that the world should process of it's own
//! accord based on a timestamp. For example: if running as a server, tick the
//! simulation along based on the `dt` passed to the plugin.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use glam::{vec3, vec4, Vec3};
use input::wire::InputState;
use input::Button;
use logger::{error, info, trace, LogLevel, Logger};
use rapier3d::control::{DynamicRayCastVehicleController, WheelTuning};
use rapier3d::na::{self as nalgebra, point, vector, Vector};
use rapier3d::prelude::{
    ColliderBuilder, ColliderHandle, ColliderSet, RigidBodyBuilder, RigidBodySet,
};
use stable_typeid::StableTypeId;
use world::bundles::StaticObject;
use world::components::spatial::SpatialHierarchyNode;
use world::components::{Camera, Control, PhysicsBody, WorldTransform};
use world::graphics::Shape;
use world::{Entity, World, WorldError};

/// Internal plugin state. The lifespan is load->update->unload and dropped
/// after unload.
pub struct WorldUpdate {
    logger: Logger,
    rigid_bodies: RigidBodySet,
    colliders: ColliderSet,
    vehicle_controller: Option<DynamicRayCastVehicleController>,
    collider_handles: HashMap<world::Entity, ColliderHandle>,
}

impl WorldUpdate {
    pub fn new() -> Self {
        Self {
            logger: LogLevel::Info.logger().sub("world-update"),
            rigid_bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            // impulse_joints: ImpulseJointSet::new(),
            // multibody_joints: MultibodyJointSet::new(),
            vehicle_controller: None,
            collider_handles: HashMap::new(),
        }
    }

    pub fn load(&mut self, world: &mut World) {
        info!(world.logger, "loaded.");
        self.logger.maybe_set_filter(world.logger.get_filter());

        // Set up colliders and rigid bodies from the world state
        self.setup_ground_collider(world);

        // Vehicle we will control manually.
        self.setup_vehicle();

        self.setup_object_colliders(world);
    }

    pub fn update(&mut self, world: &mut World, dt: &Duration) {
        let mut world = WorldExt::new(world);
        world.update_stats(dt);
        if world.is_server() {
            world.step_physical();
        }

        self.update_transform_hierarchy(&mut world);
    }

    pub fn unload(&mut self, _world: &mut World) {
        // TODO unloading things that were put into the world on load
        info!(self.logger, "unloaded");
    }
}

impl WorldUpdate {
    /// For every child in the tree, walk it's ancestors and update it's world
    /// transform from them.
    fn update_transform_hierarchy(&self, world: &mut WorldExt) {
        // TODO: is it worth marking entities dirty and re-iterating the list of updated
        // ones
        let world_transforms_updated = self.update_hierarchy(world);
        mark_clean_updated_nodes(world, &world_transforms_updated);
    }

    fn update_hierarchy(&self, world: &mut WorldExt) -> Vec<Entity> {
        let root_entity = world.world.root.unwrap();
        let mut root_transform = world
            .world
            .hecs_world
            .query_one::<(&WorldTransform,)>(root_entity)
            .unwrap();

        let (root_transform,) = root_transform.get().unwrap();
        let mut parents_query = world.world.hecs_world.query::<&SpatialHierarchyNode>();
        let parents = parents_query.view();

        let mut world_transforms_updated = vec![];
        for (entity, (node, entity_world_transform)) in world
            .world
            .hecs_world
            .query::<(&SpatialHierarchyNode, &mut WorldTransform)>()
            .iter()
            .filter(|(_entity, (node, _))| node.is_dirty())
        {
            let mut relative_matrix = node.transform;
            let mut ancestor = node.parent;
            while let Some(next) = parents.get(ancestor) {
                relative_matrix = next.transform * relative_matrix;
                ancestor = next.parent;
            }
            world_transforms_updated.push(entity);
            entity_world_transform.world = root_transform.world * relative_matrix;
        }
        if !world_transforms_updated.is_empty() {
            trace!(
                self.logger,
                "updated {} world transforms",
                world_transforms_updated.len()
            );
        }
        world_transforms_updated
    }

    fn setup_vehicle(&mut self) {
        let hw = 0.3;
        let hh = 0.15;
        let rigid_body = RigidBodyBuilder::dynamic().translation(vector![0.0, 1.0, 0.0]);
        let vehicle_handle = self.rigid_bodies.insert(rigid_body);
        let collider = ColliderBuilder::cuboid(hw * 2.0, hh, hw).density(100.0);
        self.colliders
            .insert_with_parent(collider, vehicle_handle, &mut self.rigid_bodies);

        let mut tuning = WheelTuning::default();
        tuning.suspension_stiffness = 100.0;
        tuning.suspension_damping = 10.0;
        let mut vehicle = DynamicRayCastVehicleController::new(vehicle_handle);
        let wheel_positions = [
            point![hw * 1.5, -hh, hw],
            point![hw * 1.5, -hh, -hw],
            point![-hw * 1.5, -hh, hw],
            point![-hw * 1.5, -hh, -hw],
        ];

        for pos in wheel_positions {
            vehicle.add_wheel(pos, -Vector::y(), Vector::z(), hh, hh / 4.0, &tuning);
        }
        self.vehicle_controller = Some(vehicle);
    }

    // create colliders for all objects that have a phyiscal facet
    fn setup_object_colliders(&mut self, world: &mut World) {
        let rad = 0.1;
        // TODO: use physics object to set up properties of colliders
        for (entity, (spatial, physics)) in world
            .hecs_world
            .query::<(&SpatialHierarchyNode, &PhysicsBody)>()
            .iter()
        {
            let pos = spatial.get_pos();
            let x = pos.x;
            let y = pos.y;
            let z = pos.z;

            let rigid_body = RigidBodyBuilder::dynamic().translation(vector![x, y, z]);
            let handle = self.rigid_bodies.insert(rigid_body);
            let collider = ColliderBuilder::cuboid(rad, rad, rad);

            // TODO; use shape to generate debug mesh
            let shape = collider.shape.clone();

            let collider_handle =
                self.colliders
                    .insert_with_parent(collider, handle, &mut self.rigid_bodies);
            self.collider_handles.insert(entity, collider_handle);
        }
    }

    // Create ground collider
    fn setup_ground_collider(&mut self, world: &mut World) {
        let ground_size = 10.0;
        let ground_height = 1.0;
        let ground_y_offset = 1.0;
        let root = world.root.unwrap();

        let rigid_body = RigidBodyBuilder::fixed().translation(vector![
            0.0,
            -ground_height - ground_y_offset,
            0.0
        ]);
        let floor_handle = self.rigid_bodies.insert(rigid_body);
        let collider = ColliderBuilder::cuboid(ground_size, ground_height, ground_size);
        let collider_handle =
            self.colliders
                .insert_with_parent(collider, floor_handle, &mut self.rigid_bodies);

        let shape = Shape::cuboid(ground_size, ground_height, ground_size);
        let gfx = world.add_debug_mesh(shape.into_debug_mesh(vec4(1.0, 1.0, 0.0, 1.0)));

        // TODO: try out adding a debug mesh
        let ground_phys = StaticObject::new(
            gfx,
            SpatialHierarchyNode::new_at(root, vec3(0.0, -ground_height - ground_y_offset, 0.0)),
        );

        // TODO: spawn object method
        let entity = world.hecs_world.spawn(ground_phys);
        self.collider_handles.insert(entity, collider_handle);
    }
}

fn mark_clean_updated_nodes(world: &mut WorldExt, world_transforms_updated: &[Entity]) {
    for node in world
        .world
        .hecs_world
        .query::<&mut SpatialHierarchyNode>()
        .iter()
        .filter_map(|(e, node)| {
            if world_transforms_updated.contains(&e) {
                Some(node)
            } else {
                None
            }
        })
    {
        node.set_clean();
    }
}

/// A helper struct for accessing the world state in the plugin.
struct WorldExt<'a> {
    world: &'a mut World,
    logger: Logger,
}

// TODO: lift non-dymanic stuff into World
impl<'a> WorldExt<'a> {
    fn new(world: &'a mut World) -> Self {
        let logger = world.logger.sub("world-ext");
        WorldExt { world, logger }
    }

    fn is_server(&self) -> bool {
        self.world.is_server()
    }

    fn update_stats(&mut self, dt: &Duration) {
        self.world.stats.run_life += *dt;
        self.world.stats.updates += 1;
    }

    fn duration_since_last_tick(&self) -> Duration {
        let now = Instant::now();
        let since_last_tick = now.duration_since(self.world.stats.last_tick);
        since_last_tick
    }

    fn set_last_tick(&mut self, now: Instant) {
        self.world.stats.last_tick = now;
    }

    fn step_physical(&mut self) {
        let since_last_tick = self.duration_since_last_tick();
        let action_scale = since_last_tick.as_micros() as f32 / 1000.0 / 1000.0;
        if since_last_tick > World::SIM_TICK_DELAY {
            //
            // TODO: deal with hardcoded players
            //
            if let Some(server_controller) = self.world.server_controller_state {
                let entity = self.world.player(0).unwrap();
                if let Err(err) =
                    self.move_camera_based_on_controller_state(&server_controller, entity)
                {
                    error!(self.logger, "Do any entities have a camera?");
                    let entitites = self.world.hecs_world.iter();
                    for eref in entitites {
                        error!(
                            self.logger,
                            "entity={:?} has Camera={:?} camera typeid= {:?}",
                            eref.entity(),
                            eref.has::<Camera>(),
                            StableTypeId::of::<Camera>(),
                        );
                    }
                    error!(
                        self.logger,
                        "error moving server camera: ({:?}) {:?}", entity, err
                    );
                    panic!("exiting...");
                }
            }
            if let Some(client_controller) = self.world.client_controller_state {
                let entity = self.world.player(1).unwrap();
                if let Err(err) =
                    self.move_camera_based_on_controller_state(&client_controller, entity)
                {
                    error!(self.logger, "Do any entities have a camera?");
                    let entitites = self.world.hecs_world.iter();
                    for eref in entitites {
                        error!(
                            self.logger,
                            "entity={:?} has Camera={:?}",
                            eref.entity(),
                            eref.has::<Camera>()
                        );
                    }
                    error!(
                        self.logger,
                        "error moving client camera: ({:?}) {:?}", entity, err
                    );
                }
            }
            for (_entity, (control, spatial)) in self
                .world
                .hecs_world
                .query::<(&Control, &mut SpatialHierarchyNode)>()
                .iter()
            {
                let linear = control.linear_intention * action_scale;
                spatial.translate(linear);

                let angular = control.angular_intention * action_scale;
                spatial.local_rotate(angular);
            }
        }
        self.set_last_tick(Instant::now());
    }

    fn move_camera_based_on_controller_state(
        &mut self,
        controller: &InputState,
        entity: Entity,
    ) -> Result<(), WorldError> {
        let mut query = self
            .world
            .hecs_world
            .query_one::<(&mut Camera, &mut Control, &WorldTransform, &PhysicsBody)>(entity)
            .map_err(WorldError::NoSuchEntity)?;

        let (camera, control, spatial, physics) = query.get().ok_or(WorldError::NoSuchCamera)?;

        let speed = if controller.is_button_pressed(Button::Cancel) {
            5.0
        } else {
            2.0
        };

        if self.world.stats.updates % 120 == 0 && entity == self.world.player(0).unwrap() {
            info!(
                self.logger,
                "control pos {:?} angles {:?}",
                spatial.get_pos(),
                spatial.get_angles()
            );
        }

        let forward = spatial.forward();

        if controller.is_button_pressed(Button::Down) {
            control.linear_intention += forward * speed;
        } else if controller.is_button_pressed(Button::Up) {
            control.linear_intention -= forward * speed;
        } else {
            control.linear_intention = Vec3::ZERO;
        }

        if controller.is_button_pressed(Button::Left) {
            control.angular_intention.y = -1.0 * speed;
        } else if controller.is_button_pressed(Button::Right) {
            control.angular_intention.y = speed;
        } else {
            control.angular_intention.y = 0.0;
        }

        camera.update_view_matrix(&spatial);

        Ok(())
    }
}
