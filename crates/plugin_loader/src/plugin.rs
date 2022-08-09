use std::marker::PhantomData;
use std::path::{Path, PathBuf};
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

    #[error("method not found {name} - {error:?}")]
    MethodNotFound {
        name: String,
        error: libloading::Error,
    },

    #[error("error closing lib {0:?}")]
    ErrorOnClose(libloading::Error),

    #[error("error opening lib {0:?}")]
    ErrorOnOpen(libloading::Error),

    #[error("update invoked when plugin unloaded")]
    UpdateNotLoaded,
}

#[derive(Debug, PartialEq)]
pub enum PluginUpdate {
    Updated,
    Unchanged,
}

///
/// We keep track of last-modified date of the file, and when it changes we
/// copy the file, along with a version counter to a temporary directory to load it from.
///
pub struct Plugin<T> {
    /// Source filename to watch
    path: PathBuf,
    lib: Option<Library>,
    modified: Duration,
    /// Keep track of how many times we've loaded,
    /// as we use this in the filename for the temp copy
    version: u64,
    name: String,
    tempdir: TempDir,
    _pd: PhantomData<T>,
}

type UpdateFn<T> = unsafe extern "C" fn(&mut T, &Duration);
type CallFn<T> = unsafe extern "C" fn(&mut T);

impl<T> Plugin<T> {
    /// Returns the defined name of the module
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Opens a plugin from the project target directory. Note that `check` must be called subsequently in order to invoke callbacks on the plugin.
    pub fn open_from_target_dir(plugin_name: &str) -> Result<Self, PluginError> {
        let filename = if cfg!(windows) {
            format!("{}/{}.dll", RELATIVE_TARGET_DIR, plugin_name)
        } else {
            format!("{}/deps/lib{}.so", RELATIVE_TARGET_DIR, plugin_name)
        };
        let path = PathBuf::from(filename);
        Self::open_at(path, plugin_name)
    }

    /// Opens a plugin at `path`, with `name`. Note that `check` must be called subsequently in order to invoke callbacks on the plugin.
    pub fn open_at(path: impl AsRef<Path>, name: &str) -> Result<Plugin<T>, PluginError> {
        let modified = Duration::from_millis(0);
        Ok(Plugin {
            path: path.as_ref().to_path_buf(),
            name: name.to_string(),
            tempdir: TempDir::new(name)?,
            modified,
            version: 0,
            lib: None,
            _pd: PhantomData::<T>,
        })
    }

    /// Check for an update of the lib on disk. Note that this is required for initial load.
    /// If there has been a change:
    /// - copy it to the tmp directory
    /// - call "unload" lifecycle event on the current mod if there is one
    /// - load the new library
    /// - call "load" lifecycle event on the newly loaded library, passing &mut State
    pub fn check(&mut self, state: &mut T) -> Result<PluginUpdate, PluginError> {
        let source = PathBuf::from(self.path.clone());
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
            let lib = unsafe { Library::new(temp_file_path).map_err(PluginError::ErrorOnOpen)? };
            self.lib = Some(lib);
            self.version += 1;
            self.call_load(state)?;
            return Ok(PluginUpdate::Updated);
        }
        Ok(PluginUpdate::Unchanged)
    }

    /// Call to the mod to update the state with the "update" lifecycle event
    pub fn call_update(
        &self,
        state: &mut T,
        delta_time: &Duration,
    ) -> Result<Duration, PluginError> {
        let start_time = Instant::now();
        match self.lib {
            None => return Err(PluginError::UpdateNotLoaded),
            Some(ref lib) => unsafe {
                // TODO: it could be that the lib fn needs to be cached.
                lib.get::<UpdateFn<T>>(UPDATE_METHOD)
                    .map(|f| f(state, delta_time))
                    .map_err(|error| PluginError::MethodNotFound {
                        name: String::from_utf8(UPDATE_METHOD.to_vec()).unwrap(),
                        error,
                    })?;
            },
        }
        log::debug!("Updated {} version {}", self.name(), self.version);
        Ok(start_time.elapsed())
    }

    /// Trigger the "load" lifecycle event
    fn call_load(&mut self, state: &mut T) -> Result<(), PluginError> {
        if let Some(ref lib) = self.lib {
            unsafe {
                // TODO: could cache the func until unload
                lib.get::<CallFn<T>>(LOAD_METHOD)
                    .map(|f| f(state))
                    .map_err(|error| PluginError::MethodNotFound {
                        name: String::from_utf8(LOAD_METHOD.to_vec()).unwrap(),
                        error,
                    })?;
            }
        }
        log::debug!("Loaded {} version {}", self.name(), self.version);
        Ok(())
    }

    /// Trigger the unload lifecycle event
    fn call_unload(&mut self, state: &mut T) -> Result<(), PluginError> {
        if let Some(ref lib) = self.lib {
            unsafe {
                // TODO: could cache the func until unload
                lib.get::<CallFn<T>>(UNLOAD_METHOD)
                    .map(|f| f(state))
                    .map_err(|error| PluginError::MethodNotFound {
                        name: String::from_utf8(UNLOAD_METHOD.to_vec()).unwrap(),
                        error,
                    })?;
            }
        }
        if let Some(lib) = self.lib.take() {
            lib.close()
                .map_err(|error_closing| PluginError::ErrorOnClose(error_closing))?;
        }
        log::debug!("Unloaded {} version {}", self.name(), self.version);
        Ok(())
    }
}

impl<T> Drop for Plugin<T> {
    fn drop(&mut self) {
        if let Some(lib) = self.lib.take() {
            let name = self.name();
            lib.close().unwrap_or_else(|e| {
                panic!("error closing plugin {} in drop() impl - {:?}", name, e)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write};

    use ::function_name::named;
    use cmd_lib::run_cmd;

    use super::*;

    fn generate_plugin_for_test(value: i32) -> String {
        let operation = format!("    *state += {value};");
        [
            "use std::time::Duration;",
            "#[no_mangle] pub extern \"C\" fn load(state: &mut i32) {",
            &operation,
            "}",
            "#[no_mangle] pub extern \"C\" fn update(state: &mut i32, _dt: &Duration) {",
            &operation,
            "}",
            "#[no_mangle] pub extern \"C\" fn unload(state: &mut i32) {",
            &operation,
            "}",
        ]
        .join("\n")
    }

    // actually compile the generated source using rustc as a dylib
    fn compile_lib(tempdir: &TempDir, plugin_source: &str) -> PathBuf {
        let mut source_file_path = tempdir.path().to_path_buf();
        source_file_path.push(format!("test_plugin_source.rs"));
        let mut dest_file_path = tempdir.path().to_path_buf();
        dest_file_path.push("test_plugin.plugin");

        let mut file = File::create(&source_file_path).unwrap();
        file.write_all(plugin_source.as_bytes()).unwrap();
        file.flush().unwrap();
        drop(file);

        run_cmd!(rustc ${source_file_path} --crate-type dylib -o ${dest_file_path}).unwrap();
        dest_file_path
    }

    #[test]
    #[named]
    fn test_generated_plugin() {
        cmd_lib::init_builtin_logger();
        let tempdir = TempDir::new(function_name!()).unwrap();
        let src = generate_plugin_for_test(1);
        let plugin_path = compile_lib(&tempdir, &src);

        // The normal use case - load a plugin, pass in state, then reload.
        let mut state = 1i32;
        let mut loader = Plugin::<i32>::open_at(plugin_path, "test_plugin").unwrap();
        let update = loader.check(&mut state).unwrap();
        assert_eq!(state, 2);
        assert_eq!(update, PluginUpdate::Updated);

        let dt = Duration::from_millis(1);

        loader.call_update(&mut state, &dt).unwrap();
        assert_eq!(state, 3);

        // re-generate source code for plugin, saving at the same location.
        let src = generate_plugin_for_test(-1);
        compile_lib(&tempdir, &src);

        loader.check(&mut state).unwrap();
        assert_eq!(update, PluginUpdate::Updated);

        loader.call_update(&mut state, &dt).unwrap();
        assert_eq!(state, 2);

        loader.call_update(&mut state, &dt).unwrap();
        assert_eq!(state, 1);

        loader.call_update(&mut state, &dt).unwrap();
        assert_eq!(state, 0);
    }

    #[test]
    #[named]
    fn should_fail_to_update_when_not_yet_loaded() {
        cmd_lib::init_builtin_logger();
        let tempdir = TempDir::new(function_name!()).unwrap();
        let src = generate_plugin_for_test(1);
        let plugin_path = compile_lib(&tempdir, &src);

        // The normal use case - load a plugin, pass in state, then reload.
        let mut state = 1i32;
        let loader = Plugin::<i32>::open_at(plugin_path, "test_plugin").unwrap();
        assert!(matches!(
            loader.call_update(&mut state, &Duration::from_millis(1)),
            Err(PluginError::UpdateNotLoaded)
        ));
    }

    #[test]
    fn should_fail_to_load_lib_that_doesnt_exist() {
        let mut state = 0;
        let mut loader = Plugin::<u32>::open_from_target_dir("mod_unknown").unwrap();
        assert!(matches!(loader.check(&mut state), Err(PluginError::Io(_))))
    }
}
