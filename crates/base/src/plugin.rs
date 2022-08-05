use std::marker::PhantomData;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTimeError, UNIX_EPOCH};
use std::{fs, io};

use libloading::Library;
use tempdir::TempDir;

include!(concat!(env!("OUT_DIR"), "/const_gen.rs"));

const UPDATE_METHOD: &[u8] = b"update";
const LOAD_METHOD: &[u8] = b"load";
const UNLOAD_METHOD: &[u8] = b"unload";

//
// TODO:
//     Add async futures layer over this - allowing module calls to be composed
//     together as futures.
//
//     (*)- Perhaps load modules into an evmap for lock-free concurrency?
//
// TODO: support a dynamically *defined* and dynamically loaded lib
// --> Load module definitions at runtime, even watch a mod folder and load them based on a def
//
// Plugin support:
//
// Each plugin defines a set of extern "C" functions that are called
// at specific lifecycle points.

#[derive(thiserror::Error, Debug)]
pub enum PluginError {
    #[error("io error {0:?}")]
    Io(#[from] io::Error),

    #[error("system time error {0:?}")]
    SystemTime(#[from] SystemTimeError),

    #[error("libloading error {0:?}")]
    LibLoading(#[from] libloading::Error),
}

///
/// We keep track of last-modified date of the file, and when it changes we
/// copy the file, along with a version counter to a temporary directory to load it from.
///
pub struct Plugin<T> {
    /// Source filename to watch
    filename: String,
    lib: Option<Library>,
    modified: Duration,
    /// Keep track of how many times we've loaded,
    /// as we use this in the filename for the temp copy
    version: u64,
    mod_name: String,
    tempdir: TempDir,
    _pd: PhantomData<T>,
}

type UpdateFn<T> = unsafe extern "C" fn(&mut T, &Duration);
type CallFn<T> = unsafe extern "C" fn(&mut T);

impl<T> Plugin<T> {
    ///
    /// Returns the defined name of the module
    ///
    pub fn name(&self) -> &str {
        &self.mod_name
    }

    ///
    /// Construct a new wrapper for a dynamically loaded plugin
    ///
    pub fn open(plugin_name: &str) -> Result<Self, PluginError> {
        let filename = if cfg!(windows) {
            format!("{}/{}.dll", RELATIVE_TARGET_DIR, plugin_name)
        } else {
            format!("{}/deps/lib{}.so", RELATIVE_TARGET_DIR, plugin_name)
        };
        let modified = Duration::from_millis(0);
        Ok(Plugin {
            filename: filename.to_string(),
            lib: None,
            version: 0,
            mod_name: plugin_name.to_string(),
            modified,
            tempdir: TempDir::new(plugin_name)?,
            _pd: PhantomData::<T>,
        })
    }

    ///
    /// Check for an update of the lib on disk.
    /// If there has been a change:
    /// - copy it to the tmp directory
    /// - call "unload" lifecycle event on the current mod if there is one
    /// - load the new library
    /// - call "load" lifecycle event on the newly loaded library, passing &mut State
    ///
    pub fn check_for_plugin_update(&mut self, state: &mut T) -> Result<(), PluginError> {
        let source = PathBuf::from(self.filename.clone());
        let file_stem = source.file_stem().unwrap().to_str().unwrap();

        let new_meta = fs::metadata(&source)?;
        let last_modified: Duration = new_meta.modified()?.duration_since(UNIX_EPOCH)?;
        if self.modified != last_modified {
            if self.lib.is_some() {
                self.call_unload(state)?;
            }
            self.modified = last_modified;
            let new_filename = format!("{}_{}.plugin", file_stem, self.version);
            let mut temp_file_path = self.tempdir.path().to_path_buf();
            temp_file_path.push(&new_filename);
            fs::copy(&source, temp_file_path.as_path())?;
            unsafe {
                let lib = Library::new(temp_file_path)?;
                self.version += 1;
                self.lib = Some(lib);
                self.call_load(state);
            }
        }
        Ok(())
    }

    ////
    /// update()
    ///
    /// Call to the mod to update the state with the "update" normative lifecycle event
    ///
    pub fn call_update(&mut self, state: &mut T, delta_time: &Duration) -> Duration {
        let start_time = Instant::now();
        match self.lib {
            Some(ref lib) => unsafe {
                // TODO: it could be that the lib fn needs to be cached.
                let maybe_func = lib.get::<UpdateFn<T>>(UPDATE_METHOD);
                match maybe_func {
                    Ok(func) => func(state, delta_time),
                    Err(_) => log::error!(
                        "Unable to call function: {} - method does not exist in lib: {:?}",
                        std::str::from_utf8(UPDATE_METHOD).unwrap(),
                        lib
                    ),
                }
            },
            None => log::error!(
                "Cannot call method {} - lib not found",
                std::str::from_utf8(UPDATE_METHOD).unwrap()
            ),
        }

        let elapsed = start_time.elapsed();
        log::info!("Updated {}", self.name());
        elapsed
    }

    ///
    /// call_load()
    ///
    /// Trigger the "load" lifecycle event
    ///
    fn call_load(&self, state: &mut T) {
        self.call(LOAD_METHOD, state);
        log::info!("Loaded {}", self.name());
    }

    ///
    /// call_unload()
    ///
    /// Trigger the unload lifecycle event
    ///
    fn call_unload(&mut self, state: &mut T) -> Result<(), PluginError> {
        self.call(UNLOAD_METHOD, state);
        if let Some(lib) = self.lib.take() {
            lib.close()?;
        }
        log::info!("Unloaded {}", self.name());
        Ok(())
    }

    /// call a given method by name, passing state
    fn call(&self, method: &[u8], state: &mut T) {
        match self.lib {
            Some(ref lib) => unsafe {
                // TODO: could cache the func until unload
                let maybe_func = lib.get::<CallFn<T>>(method);
                match maybe_func {
                    Ok(func) => func(state),
                    Err(e) => {
                        log::error!(
                            "Unable to call function: {} - method does not exist in lib: {:?} - {:?}",
                            std::str::from_utf8(method).unwrap(), lib, e
                        )
                    }
                }
            },
            None => log::error!(
                "Cannot call method {} - lib not found",
                std::str::from_utf8(method).unwrap()
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write};

    use cmd_lib::run_cmd;

    use super::*;

    // TODO: compile, run, and modify at runtime, to test libloader further
    const MOD_TEST_SRC: &str = r#"
use std::time::Duration;
#[no_mangle] pub extern "C" fn mod_load(state: &mut u32) {}
#[no_mangle] pub extern "C" fn mod_update(state: &mut CoreState, dt: &Duration) {}
#[no_mangle] pub extern "C" fn mod_unload(state: &mut CoreState) {}
"#;

    fn compile_lib(test_prefix: &str) {
        let tempdir = TempDir::new(test_prefix).unwrap();
        let mut source_file_path = tempdir.path().to_path_buf();
        source_file_path.push("plugin.rs");
        let mut dest_file_path = tempdir.path().to_path_buf();
        dest_file_path.push("plugin.so");

        let mut file = File::open(&source_file_path).unwrap();
        file.write_all(MOD_TEST_SRC.as_bytes()).unwrap();
        file.flush().unwrap();
        drop(file);

        run_cmd!(rustc --crate-type cdylib ${source_file_path.display()} ${dest_file_path.display()})
            .unwrap();
    }

    #[test]
    fn should_open_and_check_lib() {
        let mut state = 1;
        let mut loader = Plugin::<u32>::open("mod_test").unwrap();
        loader.check_for_plugin_update(&mut state).unwrap();
    }

    #[test]
    fn should_call_load() {
        let mut state = 1;
        let mut loader = Plugin::<u32>::open("mod_test").unwrap();
        loader.check_for_plugin_update(&mut state).unwrap();
        loader.call_load(&mut state);
    }

    #[test]
    fn should_call_unload() {
        let mut state = 1;
        let mut loader = Plugin::<u32>::open("mod_test").unwrap();
        loader.check_for_plugin_update(&mut state).unwrap();
        loader.call_load(&mut state);
        loader.call_unload(&mut state).unwrap();
        loader.call_load(&mut state);
        loader.call_unload(&mut state).unwrap();
    }

    #[test]
    fn should_fail_to_load_lib_that_doesnt_exist() {
        let mut state = 0;
        let mut loader = Plugin::<u32>::open("mod_unknown").unwrap();
        assert!(matches!(
            loader.check_for_plugin_update(&mut state),
            Err(PluginError::Io(_))
        ))
    }
}
