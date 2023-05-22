//! Plugin: `world_update_plugin`
//! Implements a plugin (see crates/plugin-loader) for prototyping 'world
//! updates'. This means anything that the world should process of it's own
//! accord based on a timestamp. For example: if running as a server, tick the
//! simulation along based on the `dt` passed to the plugin.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use glam::{Mat4, Vec3, Vec4};
use input::wire::InputState;
use input::Button;
use logger::{info, LogLevel, Logger};
use plugin_self::{impl_plugin_static, PluginState};
use rapier3d::control::{DynamicRayCastVehicleController, WheelTuning};
use rapier3d::na::{self as nalgebra, point, vector, Vector};
use rapier3d::prelude::{
    ColliderBuilder, ColliderHandle, ColliderSet, RigidBodyBuilder, RigidBodySet,
};
use world::archetypes::Archetype;
use world::{World, WorldError};

/// Internal plugin state. The lifespan is load->update->unload and dropped
/// after unload.
struct WorldUpdatePluginState {
    logger: Logger,
    rigid_bodies: RigidBodySet,
    colliders: ColliderSet,
    vehicle_controller: Option<DynamicRayCastVehicleController>,
    collider_handles: HashMap<u32, ColliderHandle>,
}

// Hang any state for this plugin off a private static within.
impl_plugin_static!(WorldUpdatePluginState, World);

impl PluginState for WorldUpdatePluginState {
    type State = World;

    fn new() -> Box<Self>
    where
        Self: Sized,
    {
        Box::new(WorldUpdatePluginState {
            logger: LogLevel::Info.logger().sub("world-update"),
            rigid_bodies: RigidBodySet::new(),
            colliders: ColliderSet::new(),
            // impulse_joints: ImpulseJointSet::new(),
            // multibody_joints: MultibodyJointSet::new(),
            vehicle_controller: None,
            collider_handles: HashMap::new(),
        })
    }

    fn load(&mut self, world: &mut Self::State) {
        info!(world.logger, "loaded.");

        // Set up colliders and rigid bodies from the world state
        self.setup_ground_collider(world);

        // Vehicle we will control manually.
        self.setup_vehicle();

        self.setup_object_colliders(world);
    }

    fn update(&mut self, world: &mut Self::State, dt: &Duration) {
        let mut world = WorldExt::new(world);
        world.update_stats(dt);
        if world.is_server() {
            world.step_physical();
        }
    }

    fn unload(&mut self, _world: &mut Self::State) {
        // TODO unloading things that were put into the world on load
        info!(self.logger, "unloaded");
    }
}

impl WorldUpdatePluginState {
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
        for physical in world.players.physics_iter_mut() {
            let x = physical.pos.x;
            let y = physical.pos.y;
            let z = physical.pos.z;

            let rigid_body = RigidBodyBuilder::dynamic().translation(vector![x, y, z]);
            let handle = self.rigid_bodies.insert(rigid_body);
            let collider = ColliderBuilder::cuboid(rad, rad, rad);
            let shape = collider.shape.clone();

            let collider_handle =
                self.colliders
                    .insert_with_parent(collider, handle, &mut self.rigid_bodies);
            //self.collider_handles.insert(phys_index, collider_handle);
        }
    }

    // Create ground collider
    fn setup_ground_collider(&mut self, world: &mut World) {
        let ground_size = 10.0;
        let ground_height = 1.0;
        let ground_y_offset = 1.0;

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

        // TODO: try out adding a debug mesh
        let ground_phys = StaticObject::new(
            0.0,
            -ground_height - ground_y_offset,
            0.0,
            5.0,
            Shape::cuboid(ground_size, ground_height, ground_size),
        );
        let debug_mesh = ground_phys
            .shape
            .into_debug_mesh(Vec4::new(1.0, 1.0, 0.0, 1.0));
        let graphics = Graphic::DebugMesh(debug_mesh);
        let ground_phys_index = world.add_physical(ground_phys);

        // THE thing. This is an entity/ game object. It exists within the world graph.
        let thing_id = world
            .add_thing(Thing::model(ground_phys_index, gfx_index))
            .unwrap();
        println!("ground collider is thing_id: {:?}", thing_id);
        self.collider_handles.insert(thing_id, collider_handle);
    }
}

/// A helper struct for accessing the world state in the plugin.
struct WorldExt<'a> {
    inner: &'a mut World,
}

// TODO: lift non-dymanic stuff into World
impl<'a> WorldExt<'a> {
    fn new(inner: &'a mut World) -> Self {
        WorldExt { inner }
    }

    fn is_server(&self) -> bool {
        self.inner.is_server()
    }

    fn update_stats(&mut self, dt: &Duration) {
        self.inner.stats.run_life += *dt;
        self.inner.stats.updates += 1;
    }

    fn duration_since_last_tick(&self) -> Duration {
        let now = Instant::now();
        let since_last_tick = now.duration_since(self.inner.stats.last_tick);
        since_last_tick
    }

    fn set_last_tick(&mut self, now: Instant) {
        self.inner.stats.last_tick = now;
    }

    fn step_physical(&mut self) {
        let since_last_tick = self.duration_since_last_tick();
        let action_scale = since_last_tick.as_micros() as f32 / 1000.0 / 1000.0;
        if since_last_tick > World::SIM_TICK_DELAY {
            if let Some(server_controller) = self.inner.server_controller_state {
                self.move_camera_based_on_controller_state(&server_controller, 0u32.into())
                    .unwrap();
            }
            if let Some(client_controller) = self.inner.client_controller_state {
                self.move_camera_based_on_controller_state(&client_controller, 1u32.into())
                    .unwrap();
            }
            for physical in self.inner.players.physics_iter_mut() {
                let linear = *physical.linear_velocity_intention * action_scale;
                *physical.pos += linear;

                let angular = *physical.angular_velocity_intention * action_scale;
                *physical.angles += angular;
            }
        }
        self.set_last_tick(Instant::now());
    }

    fn move_camera_based_on_controller_state(
        &mut self,
        controller: &InputState,
        thing_id: PlayerIndex,
    ) -> Result<(), WorldError> {
        let mut player = self.inner.player(thing_id)?;

        // TODO: move the get_camera_facet method up into World, and use that here.
        // kludge! this relies on the first two phys facets being the cameras 0,1
        // a speed-up 'run' effect if cancel is held down while moving
        let speed = if controller.is_button_pressed(Button::Cancel) {
            5.0
        } else {
            2.0
        };

        // FOR NOW: this works ok but needs work.
        // The crux here is to push changes into World from bouncing it off the physics
        // sim, but update the simulation with positions at certain points

        let rot = Mat4::from_euler(
            EULER_ROT_ORDER,
            player.angles.x,
            player.angles.y,
            player.angles.z,
        );
        let forward = rot.transform_vector3(Vec3::new(0.0, 0.0, 1.0));
        if controller.is_button_pressed(Button::Down) {
            let transform = *player.view * Mat4::from_scale(Vec3::new(1.0, 1.0, 1.0) * speed);
            *player.linear_velocity_intention += transform.transform_vector3(forward);
        } else if controller.is_button_pressed(Button::Up) {
            let transform =
                *player.view * Mat4::from_scale(-1.0 * (Vec3::new(1.0, 1.0, 1.0) * speed));
            *player.linear_velocity_intention += transform.transform_vector3(forward);
        } else {
            *player.linear_velocity_intention = Vec3::ZERO;
        }

        if controller.is_button_pressed(Button::Left) {
            player.angular_velocity_intention.y = -1.0 * speed;
        } else if controller.is_button_pressed(Button::Right) {
            player.angular_velocity_intention.y = speed;
        } else {
            player.angular_velocity_intention.y = 0.0;
        }
        player.update_view_matrix();

        Ok(())
    }
}
