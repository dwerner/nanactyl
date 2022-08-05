use std::time::Duration;

use base::CoreState;

#[no_mangle]
pub extern "C" fn load(_state: &mut CoreState) {}

#[no_mangle]
pub extern "C" fn update(_state: &mut CoreState, _dt: &Duration) {}

#[no_mangle]
pub extern "C" fn unload(_state: &mut CoreState) {}
