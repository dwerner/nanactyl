use std::time::Duration;

use base::CoreState;

#[no_mangle]
pub extern "C" fn mod_test_load(_state: &mut CoreState) {}

#[no_mangle]
pub extern "C" fn mod_test_update(_state: &mut CoreState, _dt: &Duration) {}

#[no_mangle]
pub extern "C" fn mod_test_unload(_state: &mut CoreState) {}
