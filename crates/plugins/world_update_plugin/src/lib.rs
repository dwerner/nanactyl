//! Plugin: `world_update_plugin`
//! Implements a plugin (see crates/plugin-loader) for prototyping 'world
//! updates'. This means anything that the world should process of it's own
//! accord based on a timestamp. For example: if running as a server, tick the
//! simulation along based on the `dt` passed to the plugin.

use std::time::{Duration, Instant};

use glam::{Mat4, Vec3, Vec3Swizzles};
use input::wire::InputState;
use input::Button;
use logger::{info, LogLevel, Logger};
use plugin_self::{impl_plugin, StatefulPlugin};
use world::thing::EULER_ROT_ORDER;
use world::{Identity, World, WorldError};

struct WorldUpdatePlugin {
    logger: Logger,
}

// The plugin is stateful and attached to the World struct as a Box<dyn
// StatefulPlugin> in it's update_plugin_state field.
impl_plugin!(WorldUpdatePlugin, World => update_plugin_state);

impl StatefulPlugin for WorldUpdatePlugin {
    type State = World;

    fn new() -> Box<Self>
    where
        Self: Sized,
    {
        Box::new(WorldUpdatePlugin {
            logger: LogLevel::Info.logger().sub("world-update"),
        })
    }

    fn load(&mut self, world: &mut Self::State) {
        info!(world.logger, "loaded");
    }

    fn update(&mut self, world: &mut Self::State, dt: &Duration) {
        let mut world = WorldExt { inner: world };
        world.update_stats(dt);
        if world.is_server() {
            world.step_physical(&world.duration_since_last_tick());
            world.set_last_tick(Instant::now());
        }
    }

    fn unload(&mut self, world: &mut Self::State) {
        info!(self.logger, "unloaded");
    }
}

/// A helper struct for accessing the world state in the plugin.
struct WorldExt<'a> {
    inner: &'a mut World,
}

// TODO: lift non-dymanic stuff into World
impl<'a> WorldExt<'a> {
    fn is_server(&self) -> bool {
        self.inner.is_server()
    }

    fn update_stats(&mut self, dt: &Duration) {
        self.inner.run_life += *dt;
        self.inner.updates += 1;
    }

    fn step_physical(&mut self, since_last_tick: &Duration) {
        let action_scale = since_last_tick.as_micros() as f32 / 1000.0 / 1000.0;
        if since_last_tick > &World::SIM_TICK_DELAY {
            if let Some(server_controller) = self.inner.server_controller_state {
                self.move_camera_based_on_controller_state(&server_controller, 0u32.into())
                    .unwrap();
            }
            if let Some(client_controller) = self.inner.client_controller_state {
                self.move_camera_based_on_controller_state(&client_controller, 1u32.into())
                    .unwrap();
            }
            for physical in self.inner.facets.physical.iter_mut() {
                let linear = physical.linear_velocity * action_scale;
                physical.position += linear;

                let angular = physical.angular_velocity * action_scale;
                physical.angles += angular;
            }
        }
    }

    fn duration_since_last_tick(&self) -> Duration {
        let now = Instant::now();
        let since_last_tick = now.duration_since(self.inner.last_tick);
        since_last_tick
    }

    fn set_last_tick(&mut self, now: Instant) {
        self.inner.last_tick = now;
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

        let rot = Mat4::from_euler(EULER_ROT_ORDER, pcam.angles.x, pcam.angles.y, pcam.angles.z);
        let forward = rot.transform_vector3(Vec3::new(0.0, 0.0, 1.0));
        if controller.is_button_pressed(Button::Down) {
            let transform = cam.view * Mat4::from_scale(Vec3::new(1.0, 1.0, 1.0) * speed);
            pcam.linear_velocity += transform.transform_vector3(forward);
        } else if controller.is_button_pressed(Button::Up) {
            let transform = cam.view * Mat4::from_scale(-1.0 * (Vec3::new(1.0, 1.0, 1.0) * speed));
            pcam.linear_velocity += transform.transform_vector3(forward);
        } else {
            pcam.linear_velocity = Vec3::ZERO;
        }

        if controller.is_button_pressed(Button::Left) {
            pcam.angular_velocity.y = -1.0 * speed;
        } else if controller.is_button_pressed(Button::Right) {
            pcam.angular_velocity.y = speed;
        } else {
            pcam.angular_velocity.y = 0.0;
        }
        cam.update_view_matrix(pcam);

        Ok(())
    }
}
