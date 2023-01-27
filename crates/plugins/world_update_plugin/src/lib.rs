//! Plugin: `world_update_plugin`
//! Implements a plugin (see crates/plugin-loader) for prototyping 'world
//! updates'. This means anything that the world should process of it's own
//! accord based on a timestamp. For example: if running as a server, tick the
//! simulation along based on the `dt` passed to the plugin.

use std::time::{Duration, Instant};

use input::wire::InputState;
use input::Button;
use world::{Identity, Matrix4, Vector3, World, WorldError};

#[no_mangle]
pub extern "C" fn load(world: &mut World) {
    println!("loaded world update plugin ({})!", world.updates);
}

#[no_mangle]
pub extern "C" fn update(world: &mut World, dt: &Duration) {
    world.run_life += *dt;
    world.updates += 1;

    if world.is_server() {
        let now = Instant::now();
        let since_last_tick = now.duration_since(world.last_tick);
        let action_scale = since_last_tick.as_micros() as f32 / 1000.0 / 1000.0;
        if since_last_tick > World::SIM_TICK_DELAY {
            {
                if let Some(server_controller) = world.server_controller_state {
                    move_camera_based_on_controller_state(world, &server_controller, 0u32.into())
                        .unwrap();
                }
                if let Some(client_controller) = world.client_controller_state {
                    move_camera_based_on_controller_state(world, &client_controller, 1u32.into())
                        .unwrap();
                }
            }
            for physical in world.facets.physical.iter_mut() {
                let linear = physical.linear_velocity * action_scale;
                physical.position += linear;

                let angular = physical.angular_velocity * action_scale;
                physical.angles += angular;
            }
            world.last_tick = Instant::now();
        }
    } else {
        // try to predict, but dont be suprised if an update corrects it
        // (rubber-banding tho)
    }
}

#[no_mangle]
pub extern "C" fn unload(world: &mut World) {
    println!("unloaded world update plugin ({})", world.updates);
}

fn move_camera_based_on_controller_state(
    world: &mut World,
    controller: &InputState,
    thing_id: Identity,
) -> Result<(), WorldError> {
    let (phys_idx, cam_idx) = world.camera_facet_indices(thing_id)?;

    let mut cam = world
        .facets
        .camera_mut(cam_idx)
        .ok_or_else(|| WorldError::NoSuchCamera(cam_idx))?
        .clone();

    let pcam = world
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

    let rot = Matrix4::new_rotation(-1.0 * pcam.angles);
    let forward = rot.transform_vector(&Vector3::new(0.0, 0.0, 1.0));
    if controller.is_button_pressed(Button::Down) {
        let transform = cam.view * Matrix4::new_scaling(-1.0 * speed);
        pcam.linear_velocity += transform.transform_vector(&forward);
    } else if controller.is_button_pressed(Button::Up) {
        let transform = cam.view * Matrix4::new_scaling(speed);
        pcam.linear_velocity += transform.transform_vector(&forward);
    } else {
        pcam.linear_velocity = Vector3::zeros();
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
