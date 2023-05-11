//! Plugin: `world_update_plugin`
//! Implements a plugin (see crates/plugin-loader) for prototyping 'world
//! updates'. This means anything that the world should process of it's own
//! accord based on a timestamp. For example: if running as a server, tick the
//! simulation along based on the `dt` passed to the plugin.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use glam::{Mat4, Vec3};
use input::wire::InputState;
use input::Button;
use logger::{info, LogLevel, Logger};
use plugin_self::{impl_plugin, StatefulPlugin};
use rapier3d::control::{DynamicRayCastVehicleController, WheelTuning};
use rapier3d::na::{self as nalgebra, point, vector, Vector};
use rapier3d::prelude::{
    ColliderBuilder, ColliderHandle, ColliderSet, RigidBodyBuilder, RigidBodySet,
};
use world::thing::{PhysicalIndex, EULER_ROT_ORDER};
use world::{Identity, World, WorldError};

struct WorldUpdatePlugin {
    logger: Logger,
    rigid_bodies: RigidBodySet,
    colliders: ColliderSet,
    vehicle_controller: Option<DynamicRayCastVehicleController>,
    collider_handles: HashMap<PhysicalIndex, ColliderHandle>,
}

// The plugin is stateful and attached to the World struct as a Box<dyn
// StatefulPlugin> in it's update_plugin_state field. All resources owned by the
// plugin will be dropped when it is unloaded.
impl_plugin!(WorldUpdatePlugin, World => update_plugin_state);

impl StatefulPlugin for WorldUpdatePlugin {
    type State = World;

    fn new() -> Box<Self>
    where
        Self: Sized,
    {
        Box::new(WorldUpdatePlugin {
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
        info!(world.logger, "loaded");

        // Set up colliders and rigid bodies from the world state
        self.setup_ground_collider();

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
        info!(self.logger, "unloaded");
    }
}

impl WorldUpdatePlugin {
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
        for (phys_index, physical) in world.facets.iter_physical_mut() {
            let x = physical.position.x;
            let y = physical.position.y;
            let z = physical.position.z;

            let rigid_body = RigidBodyBuilder::dynamic().translation(vector![x, y, z]);
            let handle = self.rigid_bodies.insert(rigid_body);
            let collider = ColliderBuilder::cuboid(rad, rad, rad);
            let collider_handle =
                self.colliders
                    .insert_with_parent(collider, handle, &mut self.rigid_bodies);
            self.collider_handles.insert(phys_index, collider_handle);
        }
    }

    // Create ground collider
    fn setup_ground_collider(&mut self) {
        let ground_size = 5.0;
        let ground_height = 0.1;

        let rigid_body = RigidBodyBuilder::fixed().translation(vector![0.0, -ground_height, 0.0]);
        let floor_handle = self.rigid_bodies.insert(rigid_body);
        let collider = ColliderBuilder::cuboid(ground_size, ground_height, ground_size);
        self.colliders
            .insert_with_parent(collider, floor_handle, &mut self.rigid_bodies);
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
        self.inner.run_life += *dt;
        self.inner.updates += 1;
    }

    fn duration_since_last_tick(&self) -> Duration {
        let now = Instant::now();
        let since_last_tick = now.duration_since(self.inner.last_tick);
        since_last_tick
    }

    fn set_last_tick(&mut self, now: Instant) {
        self.inner.last_tick = now;
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
            for physical in self.inner.facets.physical.iter_mut() {
                let linear = physical.linear_velocity_intention * action_scale;
                physical.position += linear;

                let angular = physical.angular_velocity_intention * action_scale;
                physical.angles += angular;
            }
        }
        self.set_last_tick(Instant::now());
    }

    fn move_camera_based_on_controller_state(
        &mut self,
        controller: &InputState,
        thing_id: Identity,
    ) -> Result<(), WorldError> {
        let (phys_idx, cam_idx) = self.inner.camera_facet_indices(thing_id)?;

        let mut cam = self
            .inner
            .facets
            .camera_mut(cam_idx)
            .ok_or_else(|| WorldError::NoSuchCamera(cam_idx))?
            .clone();

        let pcam = self
            .inner
            .facets
            .physical_mut(phys_idx)
            .ok_or_else(|| WorldError::NoSuchCamera(cam_idx))?;

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

        let rot = Mat4::from_euler(EULER_ROT_ORDER, pcam.angles.x, pcam.angles.y, pcam.angles.z);
        let forward = rot.transform_vector3(Vec3::new(0.0, 0.0, 1.0));
        if controller.is_button_pressed(Button::Down) {
            let transform = cam.view * Mat4::from_scale(Vec3::new(1.0, 1.0, 1.0) * speed);
            pcam.linear_velocity_intention += transform.transform_vector3(forward);
        } else if controller.is_button_pressed(Button::Up) {
            let transform = cam.view * Mat4::from_scale(-1.0 * (Vec3::new(1.0, 1.0, 1.0) * speed));
            pcam.linear_velocity_intention += transform.transform_vector3(forward);
        } else {
            pcam.linear_velocity_intention = Vec3::ZERO;
        }

        if controller.is_button_pressed(Button::Left) {
            pcam.angular_velocity_intention.y = -1.0 * speed;
        } else if controller.is_button_pressed(Button::Right) {
            pcam.angular_velocity_intention.y = speed;
        } else {
            pcam.angular_velocity_intention.y = 0.0;
        }
        cam.update_view_matrix(pcam);

        Ok(())
    }
}
