/// A loadable plugin must define the plugin's name and version and state, as
/// well as provide methods for loading, updating, and unloading the plugin.
pub trait LoadablePlugin {
    /// The name of the plugin.
    const NAME: &'static str;

    /// The version of the plugin.
    const VERSION: u64;

    type State: Send + Sync;

    /// Load the plugin.
    ///
    /// This method is called when the plugin is loaded.
    ///
    /// # Arguments
    ///
    /// * `state`: The state that the plugin will manipulate.
    fn load(state: &mut Self::State);

    /// Update the plugin.
    ///
    /// This method is called periodically to update the state of the plugin.
    ///
    /// # Arguments
    ///
    /// * `state`: The state that the plugin will manipulate.
    /// * `delta_time`: The amount of time that has passed since the last
    ///   update.
    fn update(state: &mut Self::State, delta_time: &std::time::Duration);

    /// Unload the plugin.
    ///
    /// This method is called when the plugin is unloaded.
    ///
    /// # Arguments
    ///
    /// * `state`: The state that the plugin will manipulate.
    fn unload(state: &mut Self::State);
}

#[macro_export]
macro_rules! impl_plugin {
    ($self_ty:ty, $assoc_type:ty) => {
        #[no_mangle]
        pub extern "C" fn load(state: &mut $assoc_type) {
            <$self_ty as LoadablePlugin>::load(state)
        }

        #[no_mangle]
        pub extern "C" fn update(state: &mut $assoc_type, delta_time: &std::time::Duration) {
            <$self_ty as LoadablePlugin>::update(state, delta_time);
        }

        #[no_mangle]
        pub extern "C" fn unload(state: &mut $assoc_type) {
            <$self_ty as LoadablePlugin>::unload(state);
        }
    };
}
