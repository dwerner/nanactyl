use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTimeError, UNIX_EPOCH};
use std::{fs, io};

use async_lock::Mutex;
use core_executor::ThreadExecutorSpawner;
use libloading::Library;
use tempdir::TempDir;

include!(concat!(env!("OUT_DIR"), "/const_gen.rs"));

const ENABLE_PLUGIN_MAPPING_CHECK: bool = false;
const UPDATE_METHOD: &[u8] = b"update";
const LOAD_METHOD: &[u8] = b"load";
const UNLOAD_METHOD: &[u8] = b"unload";

#[derive(thiserror::Error, Debug)]
pub enum PluginError {
    #[error("copy file io error {0:?}")]
    CopyFile(io::Error),
    #[error("tempdir io error {0:?}")]
    TempdirIo(io::Error),

    #[error("metadata io plugin name: {name}, path {path}, err {err:?}")]
    MetadataIo {
        name: String,
        path: PathBuf,
        err: io::Error,
    },

    #[error("modified io error {0:?}")]
    ModifiedTime(io::Error),

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

#[derive(Debug, PartialEq, Eq)]
pub enum PluginCheck {
    FoundNewVersion,
    Unchanged,
}

// Plugin support:
//
// Each plugin defines a set of extern "C" functions that are called
// at specific lifecycle points.
///
/// We keep track of last-modified date of the file, and when it changes we
/// copy the file, along with a version counter to a temporary directory to load it from.
///
pub struct Plugin<T: Send + Sync + 'static> {
    /// Source filename to watch
    path: PathBuf,
    updates: u64,
    last_reloaded: u64,
    check_interval: u32,
    lib: Option<Library>,
    modified: Duration,
    libcache: Option<LibCache<T>>,
    /// Keep track of how many times we've loaded,
    /// as we use this in the filename for the temp copy
    version: u64,
    name: String,
    tempdir: TempDir,
    _spawner: ThreadExecutorSpawner,
    _pd: PhantomData<T>,
}

#[cfg(unix)]
use libloading::os::unix::Symbol as PlatformSymbol;
#[cfg(windows)]
use libloading::os::windows::Symbol as PlatformSymbol;

struct LibCache<T> {
    load: PlatformSymbol<CallFn<T>>,
    update: PlatformSymbol<UpdateFn<T>>,
    unload: PlatformSymbol<CallFn<T>>,
}

type UpdateFn<T> = unsafe extern "C" fn(&mut T, &Duration);
type CallFn<T> = unsafe extern "C" fn(&mut T);

impl<T> Plugin<T>
where
    T: Send + Sync,
{
    /// Wrap this plugin in Arc<Mutex<_>>
    pub fn into_shared(self) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(self))
    }

    /// Returns the defined name of the module
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Returns the defined name of the module
    pub fn version(&self) -> u64 {
        self.version
    }

    /// Opens a plugin from the project target directory. Note that `check` must be called subsequently in order to invoke callbacks on the plugin.
    pub fn open_from_target_dir(
        spawner: ThreadExecutorSpawner,
        plugin_dir: &str,
        plugin_name: &str,
    ) -> Result<Self, PluginError> {
        let filename = if cfg!(windows) {
            format!("{}/{}.dll", plugin_dir, plugin_name)
        } else {
            format!("{}/lib{}.so", plugin_dir, plugin_name)
        };
        let path = PathBuf::from(filename);
        Self::open_at(spawner, path, plugin_name, 120)
    }

    /// Opens a plugin at `path`, with `name`. Note that `check` must be called subsequently in order to invoke callbacks on the plugin.
    pub fn open_at(
        mut spawner: ThreadExecutorSpawner,
        path: impl AsRef<Path>,
        name: &str,
        check_interval: u32,
    ) -> Result<Plugin<T>, PluginError> {
        let modified = Duration::from_millis(0);
        let path = path.as_ref().to_path_buf();
        let name = name.to_string();
        fs::metadata(&path).map_err(|err| PluginError::MetadataIo {
            name: name.to_string(),
            path: path.clone(),
            err,
        })?;

        #[cfg(unix)]
        if ENABLE_PLUGIN_MAPPING_CHECK {
            // TODO: move this into the plugin's interface - rely on the caller to delegate the work.
            let plugin_name = name.clone();
            spawner.spawn_with_shutdown(move |mut shutdown| {
                Box::pin(async move {
                    loop {
                        let mappings = crate::linux::distinct_plugins_mapped(&plugin_name);
                        if mappings.len() > 1 {
                            let mut mappings = mappings.into_iter().collect::<Vec<_>>();
                            mappings.sort();
                            log::warn!("multiple plugins mapped for {plugin_name}:\n{mappings:#?}");
                        }
                        async_io::Timer::after(Duration::from_millis(250)).await;
                        if shutdown.should_exit() {
                            break;
                        }
                    }
                })
            });
        }

        Ok(Plugin {
            path,
            tempdir: TempDir::new(&name).map_err(PluginError::TempdirIo)?,
            name: name.to_string(),
            modified,
            version: 0,
            updates: 0,
            last_reloaded: 0,
            check_interval,
            lib: None,
            libcache: None,
            _spawner: spawner,
            _pd: PhantomData::<T>,
        })
    }

    /// Check for an update of the lib on disk. Note that this is required for initial load.
    /// If there has been a change:
    /// - copy it to the tmp directory
    /// - load the new library
    /// - call "unload" lifecycle event on the current mod if there is one
    /// - call "load" lifecycle event on the newly loaded library, passing &mut State
    pub fn check(&mut self, state: &mut T) -> Result<PluginCheck, PluginError> {
        if !self.should_check() {
            return Ok(PluginCheck::Unchanged);
        }

        let source = self.path.clone();
        let file_stem = source.file_stem().unwrap().to_str().unwrap();
        let new_meta = fs::metadata(&source).map_err(|err| PluginError::MetadataIo {
            err,
            path: source.clone(),
            name: self.name().to_string(),
        })?;

        let last_modified: Duration = new_meta
            .modified()
            .map_err(PluginError::ModifiedTime)?
            .duration_since(UNIX_EPOCH)?;

        if self.modified != last_modified {
            self.modified = last_modified;
            let new_filename = format!("{}_{}.plugin", file_stem, self.version);
            let mut temp_file_path = self.tempdir.path().to_path_buf();
            temp_file_path.push(&new_filename);
            fs::copy(&source, temp_file_path.as_path()).map_err(PluginError::CopyFile)?;
            let lib = unsafe { Library::new(temp_file_path).map_err(PluginError::ErrorOnOpen)? };
            let libcache = unsafe {
                let load = lib.get::<CallFn<T>>(LOAD_METHOD).map_err(|error| {
                    PluginError::MethodNotFound {
                        name: "load".to_string(),
                        error,
                    }
                })?;
                let unload = lib.get::<CallFn<T>>(UNLOAD_METHOD).map_err(|error| {
                    PluginError::MethodNotFound {
                        name: "unload".to_string(),
                        error,
                    }
                })?;
                let update = lib.get::<UpdateFn<T>>(UPDATE_METHOD).map_err(|error| {
                    PluginError::MethodNotFound {
                        name: "update".to_string(),
                        error,
                    }
                })?;
                LibCache {
                    load: load.into_raw(),
                    update: update.into_raw(),
                    unload: unload.into_raw(),
                }
            };
            if self.lib.is_some() {
                self.call_unload(state)?;
            }
            self.lib = Some(lib);
            self.libcache = Some(libcache);
            self.version += 1;
            self.last_reloaded = 0;
            self.call_load(state)?;
            return Ok(PluginCheck::FoundNewVersion);
        }
        Ok(PluginCheck::Unchanged)
    }

    /// Should the plugin wrapper check for a new version on disk?
    /// Also used on unix systems to determine if we should check /proc/PID/maps for plugin mappings.
    fn should_check(&self) -> bool {
        self.updates == 0
            || (self.updates > 0
                && self.updates % self.check_interval as u64 == 0
                && self.last_reloaded >= self.check_interval as u64)
    }

    /// Call to the mod to update the state with the "update" lifecycle event.
    pub async fn call_update(
        &mut self,
        state: &mut T,
        delta_time: &Duration,
    ) -> Result<Duration, PluginError> {
        let start_time = Instant::now();
        match self.libcache.as_ref() {
            None => return Err(PluginError::UpdateNotLoaded),
            Some(cache) => unsafe {
                // TODO: it could be that the lib fn needs to be cached.
                (cache.update)(state, delta_time);
            },
        }
        self.updates += 1;
        self.last_reloaded += 1;
        log::debug!(
            "Updated {} version {} (updates {}, last_reloaded {})",
            self.name(),
            self.version,
            self.updates,
            self.last_reloaded
        );

        Ok(start_time.elapsed())
    }

    /// Trigger the "load" lifecycle event
    fn call_load(&mut self, state: &mut T) -> Result<(), PluginError> {
        if let Some(cache) = self.libcache.as_ref() {
            unsafe {
                (cache.load)(state);
            }
        }
        log::debug!("Loaded {} version {}", self.name(), self.version);
        Ok(())
    }

    /// Trigger the unload lifecycle event
    fn call_unload(&mut self, state: &mut T) -> Result<(), PluginError> {
        if let Some(cache) = self.libcache.as_ref() {
            unsafe {
                (cache.unload)(state);
            }
        }
        if let Some(lib) = self.lib.take() {
            lib.close().map_err(PluginError::ErrorOnClose)?;
            self.libcache.take();
        }
        log::debug!("Unloaded {} version {}", self.name(), self.version);
        Ok(())
    }
}

impl<T> Drop for Plugin<T>
where
    T: Send + Sync,
{
    fn drop(&mut self) {
        if let Some(lib) = self.lib.take() {
            let name = self.name();
            lib.close().unwrap_or_else(|e| {
                panic!("error closing plugin {} in drop() impl - {:?}", name, e)
            });
            self.libcache.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::File;
    use std::io::Write;

    use ::function_name::named;
    use cmd_lib::run_cmd;
    use core_executor::ThreadExecutor;

    use super::*;
    use crate as plugin_loader;
    use crate::register_tls_dtor_hook;

    fn generate_plugin_for_test(global_scope: &str, operation: &str) -> String {
        [
            "use std::time::Duration;",
            global_scope,
            "#[no_mangle] pub extern \"C\" fn load(state: &mut i32) {",
            operation,
            "}",
            "#[no_mangle] pub extern \"C\" fn update(state: &mut i32, _dt: &Duration) {",
            operation,
            "}",
            "#[no_mangle] pub extern \"C\" fn unload(state: &mut i32) {",
            operation,
            "}",
        ]
        .join("\n")
    }

    // actually compile the generated source using rustc as a dylib
    fn compile_lib(tempdir: &TempDir, plugin_source: &str) -> PathBuf {
        let mut source_file_path = tempdir.path().to_path_buf();
        source_file_path.push("test_plugin_source.rs".to_string());
        let mut dest_file_path = tempdir.path().to_path_buf();
        dest_file_path.push("test_plugin.plugin");

        let mut file = File::create(&source_file_path).unwrap();
        file.write_all(plugin_source.as_bytes()).unwrap();
        file.flush().unwrap();
        drop(file);

        run_cmd!(rustc ${source_file_path} --crate-type cdylib -o ${dest_file_path}).unwrap();
        dest_file_path
    }

    #[smol_potat::test]
    #[named]
    async fn test_generated_plugin() {
        let tempdir = TempDir::new(function_name!()).unwrap();
        let src = generate_plugin_for_test("", "*state += 1;");
        let plugin_path = compile_lib(&tempdir, &src);

        let ThreadExecutor { ref spawner, .. } = ThreadExecutor::new(0);

        // The normal use case - load a plugin, pass in state, then reload.
        let mut state = 1i32;
        let mut loader =
            Plugin::<i32>::open_at(spawner.clone(), plugin_path, "test_plugin", 1).unwrap();
        let update = loader.check(&mut state).unwrap();
        assert_eq!(state, 2);
        assert_eq!(update, PluginCheck::FoundNewVersion);

        let dt = Duration::from_millis(1);

        loader.call_update(&mut state, &dt).await.unwrap();
        assert_eq!(state, 3);

        // re-generate source code for plugin, saving at the same location.
        let src = generate_plugin_for_test("", "*state -= 1;");
        compile_lib(&tempdir, &src);

        loader.check(&mut state).unwrap();
        assert_eq!(update, PluginCheck::FoundNewVersion);

        loader.call_update(&mut state, &dt).await.unwrap();
        assert_eq!(state, 2);

        loader.call_update(&mut state, &dt).await.unwrap();
        assert_eq!(state, 1);

        loader.call_update(&mut state, &dt).await.unwrap();
        assert_eq!(state, 0);
    }

    #[smol_potat::test]
    #[named]
    async fn test_generated_plugin_mappings() {
        register_tls_dtor_hook!();

        let tempdir = TempDir::new(function_name!()).unwrap();
        let src = generate_plugin_for_test("", "*state += 1;");
        let plugin_path = compile_lib(&tempdir, &src);

        let ThreadExecutor { ref spawner, .. } = ThreadExecutor::new(0);
        // The normal use case - load a plugin, pass in state, then reload.
        let mut state = 1i32;
        let mut loader =
            Plugin::<i32>::open_at(spawner.clone(), plugin_path, "test_plugin", 1).unwrap();
        let update = loader.check(&mut state).unwrap();
        assert_eq!(state, 2);
        assert_eq!(update, PluginCheck::FoundNewVersion);

        let dt = Duration::from_millis(1);

        loader.call_update(&mut state, &dt).await.unwrap();
        assert_eq!(state, 3);

        // re-generate source code for plugin, saving at the same location.
        let src = generate_plugin_for_test(
            r#"
            use std::cell::RefCell;
            thread_local! {
                pub static THING: RefCell<Option<Box<dyn std::io::Write + Send>>> = RefCell::new(None);
            }
            "#,
            r#"
            *state -= 1;
            println!("static THING {:?}", THING);
            println!("{:?}", std::thread::current().id());
        "#,
        );
        compile_lib(&tempdir, &src);

        let update = loader.check(&mut state).unwrap();
        assert_eq!(update, PluginCheck::FoundNewVersion);

        loader.call_update(&mut state, &dt).await.unwrap();
        assert_eq!(state, 2);

        #[cfg(unix)]
        assert_eq!(
            crate::linux::distinct_plugins_mapped("test_plugin"),
            ["test_plugin_1"],
        );
    }

    #[smol_potat::test]
    #[named]
    async fn should_fail_to_update_when_not_yet_loaded() {
        let tempdir = TempDir::new(function_name!()).unwrap();
        let src = generate_plugin_for_test("", "*state += 1;");
        let plugin_path = compile_lib(&tempdir, &src);

        let ThreadExecutor { ref spawner, .. } = ThreadExecutor::new(0);
        // The normal use case - load a plugin, pass in state, then reload.
        let mut state = 1i32;
        let mut loader =
            Plugin::<i32>::open_at(spawner.clone(), plugin_path, "test_plugin", 1).unwrap();
        assert!(matches!(
            loader
                .call_update(&mut state, &Duration::from_millis(1))
                .await,
            Err(PluginError::UpdateNotLoaded)
        ));
    }

    #[test]
    fn should_fail_to_load_lib_that_doesnt_exist() {
        let ThreadExecutor { ref spawner, .. } = ThreadExecutor::new(0);
        let load = Plugin::<u32>::open_from_target_dir(
            spawner.clone(),
            plugin_loader::RELATIVE_TARGET_DIR,
            "mod_unknown",
        );
        assert!(matches!(load, Err(PluginError::MetadataIo { .. })))
    }
}
