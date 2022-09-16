use std::time::Duration;

use world::World;

#[no_mangle]
pub extern "C" fn load(world: &mut World) {
    println!("loaded world update plugin ({})!", world.updates);
}

#[no_mangle]
pub extern "C" fn update(world: &mut World, dt: &Duration) {
    world.maybe_tick(dt);
}

#[no_mangle]
pub extern "C" fn unload(world: &mut World) {
    println!("unloaded world update plugin ({})", world.updates);
}
