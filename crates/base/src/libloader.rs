use std::path::Path;
use std::time::{Duration, Instant, SystemTimeError, UNIX_EPOCH};
use std::{fs, io};

use libloading::Library;

use super::CoreState;

include!(concat!(env!("OUT_DIR"), "/const_gen.rs"));

///
/// TODO:
///     Add async futures layer over this - allowing module calls to be composed
///     together as futures.
///
///     (*)- Perhaps load modules into an evmap for lock-free concurrency?
///
/// TODO: support a dynamically *defined* and dynamically loaded lib
/// --> Load module definitions at runtime, even watch a mod folder and load them based on a def
///
/// Mods support:
///
/// Mods need to be named mod_<mod-name>, and must be unique.
/// Each mod defines a set of extern "C" functions that are called
/// at specific lifecycle points.
///
/// Usage:
/// let a_mod = load_mod!(modnamehere);
/// let mut s = CoreState::new();
/// loop {
///     a_mod.check_update(&mut s);
///     a_mod.tick(&mut s);
/// }
///

///
/// Macro for loading platform-specific shared lib (dll/so)
///
#[macro_export]
macro_rules! load_mod {
    ( $s:expr ) => {{
        let name = stringify!($s);
        let path = if cfg!(windows) {
            format!("{}/{}.dll", TARGET_DIR, name,)
        } else {
            format!("{}/deps/lib{}.so", TARGET_DIR, name,)
        };
        LibLoader::new(&path, name)
    }};
}

#[derive(thiserror::Error, Debug)]
pub enum LibLoaderError {
    #[error("io error {0:?}")]
    Io(#[from] io::Error),
    #[error("system time error {0:?}")]
    SystemTime(#[from] SystemTimeError),
}

///
/// LibLoader - an instance represents the managed loading of a dynamic shared library
/// (.dll or .so, potentially in the future a .dylib)
///
/// We keep track of last-modified date of the file, and when it changes we
/// copy the file, along with a version counter to a temporary directory to load it from.
///
pub struct LibLoader {
    filename: String, // Source filename to watch
    lib: Option<Library>,
    modified: Duration,
    version: u64, // Keep track of how many times we've loaded,
    // as we use this in the filename for the temp copy
    mod_name: String,
}

impl LibLoader {
    ///
    /// Returns the defined name of the module
    ///
    pub fn name(&self) -> &str {
        &self.mod_name
    }

    ///
    /// Construct a new wrapper for a dynamically loaded mod
    ///
    pub fn new(filename: &str, mod_name: &str) -> Self {
        let modified = Duration::from_millis(0);
        LibLoader {
            filename: filename.to_string(),
            lib: None,
            version: 0,
            mod_name: mod_name.to_string(),
            modified,
        }
    }

    ///
    /// Check for an update of the lib on disk.
    /// If there has been a change:
    /// - copy it to the tmp directory
    /// - call "unload" lifecycle event on the current mod if there is one
    /// - load the new library
    /// - call "load" lifecycle event on the newly loaded library, passing &mut State
    ///
    pub fn check_update(&mut self, state: &mut CoreState) -> Result<(), LibLoaderError> {
        let source = Path::new(&self.filename);
        let file_stem = source.file_stem().unwrap().to_str().unwrap();
        let new_meta = fs::metadata(&source)?;
        let last_modified: Duration = new_meta.modified()?.duration_since(UNIX_EPOCH)?;
        if self.lib.is_none() || self.modified != last_modified {
            self.modified = last_modified;
            let new_filename = format!("target/{}_{}.so", file_stem, self.version);
            match fs::copy(&source, &new_filename) {
                Ok(_) => {
                    if self.lib.is_some() {
                        self.unload(state);
                    }
                    unsafe {
                        match Library::new(&new_filename) {
                            Ok(lib) => {
                                self.version += 1;
                                self.lib = Some(lib);
                                self.load(state);
                            }
                            Err(err) => println!(
                                "Unable to open new library: {} - err: {}",
                                new_filename, err
                            ),
                        }
                    }
                }
                Err(err) => println!(
                    "Error copying file, target: {} - err: {}",
                    new_filename, err
                ),
            }
        }

        Ok(())
    }

    ///
    /// update()
    ///
    /// Call to the mod to update the state with the "update" normative lifecycle event
    ///
    pub fn update(&self, state: &mut CoreState, delta_time: &Duration) -> Duration {
        let method_name = format!("{}_update", self.mod_name);
        let start_time = Instant::now();
        match self.lib {
            Some(ref lib) => unsafe {
                let method = method_name.as_bytes();
                // TODO: it could be that the lib fn needs to be cached.
                let maybe_func = lib.get::<unsafe extern "C" fn(&mut CoreState, &Duration)>(method);
                match maybe_func {
                    Ok(func) => func(state, delta_time),
                    Err(_) => println!(
                        "Unable to call function: {} - method does not exist in lib: {:?}",
                        method_name, lib
                    ),
                }
            },
            None => println!("Cannot call method {} - lib not found", method_name),
        }
        start_time.elapsed()
    }

    ///
    /// load()
    ///
    /// Trigger the "load" lifecycle event
    ///
    fn load(&self, state: &mut CoreState) {
        let method_name = format!("{}_load", self.mod_name);
        self.call(&method_name, state);
        self.message("Loaded");
    }

    ///
    /// unload()
    ///
    /// Trigger the unload lifecycle event
    ///
    fn unload(&self, state: &mut CoreState) {
        let method_name = format!("{}_unload", self.mod_name);
        self.call(&method_name, state);
        self.message("Unloaded");
    }

    ///
    /// message()
    ///
    /// (used to signal changes in mod versions)
    ///
    fn message(&self, message: &str) {
        let source = Path::new(&self.filename);
        let file_stem = source.file_stem().unwrap().to_str().unwrap();
        println!(
            "[{} {} (version {}, {:?})]",
            message, file_stem, self.version, source,
        );
    }

    fn call(&self, method_name: &str, state: &mut CoreState) {
        match self.lib {
            Some(ref lib) => unsafe {
                let method = method_name.as_bytes();
                // TODO: could cache the func until unload
                let maybe_func = lib.get::<unsafe extern "C" fn(&mut CoreState)>(method);
                match maybe_func {
                    Ok(func) => func(state),
                    Err(e) => println!(
                        "Unable to call function: {} - method does not exist in lib: {:?} - {:?}",
                        method_name, lib, e
                    ),
                }
            },
            None => println!("Cannot call method {} - lib not found", method_name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn should_load_lib() {
        let mut state = CoreState;
        let mut loader = load_mod!(mod_test);
        loader.check_update(&mut state).unwrap();
    }

    #[test]
    fn should_fail_to_load_lib_that_doesnt_exist() {
        let mut state = CoreState;
        let mut loader = load_mod!(mod_unknown);
        assert!(matches!(
            loader.check_update(&mut state),
            Err(LibLoaderError::Io(_))
        ))
    }
}
