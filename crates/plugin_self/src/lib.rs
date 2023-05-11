/// A loadable plugin must define the plugin's name and version and state, as
/// well as provide methods for loading, updating, and unloading the plugin from
/// the application owned state.
///
///
/// ```
/// /// In the app
/// struct State {
///     plugin_field: Box<dyn StatefulPlugin>
/// }
///
/// // In a plugin
/// struct PluginWithState {
///     // some state
/// }
///
/// impl StatefulPlugin for GameState { ... }
///
/// impl_plugin!(PluginWithState, GameState => plugin_field);
/// ```
///
/// Notes:
/// Now the app can load the plugin. Note that for reload-safety to work, the
/// app *MUST* call 'unload' on the plugin before dropping it.
///
/// Safety:
/// Must call 'unload' on the plugin before dropping it.
pub trait StatefulPlugin {
    type State: Send + Sync;

    /// Zero arg constructor.
    fn new() -> Box<Self>
    where
        Self: Sized;

    /// Load the plugin.
    ///
    /// This method is called when the plugin is loaded.
    ///
    /// # Arguments
    ///
    /// * `state`: The state that the plugin will manipulate.
    fn load(&mut self, state: &mut Self::State);

    /// Update the plugin.
    ///
    /// This method is called periodically to update the state of the plugin.
    ///
    /// # Arguments
    ///
    /// * `state`: The state that the plugin will manipulate.
    /// * `delta_time`: The amount of time that has passed since the last
    ///   update.
    fn update(&mut self, state: &mut Self::State, delta_time: &std::time::Duration);

    /// Unload the plugin.
    ///
    /// This method is called when the plugin is unloaded.
    ///
    /// # Arguments
    ///
    /// * `state`: The state that the plugin will manipulate.
    fn unload(&mut self, state: &mut Self::State);
}

/// Implements the binding for this plugin.
///
/// Notes:
/// Intended to be used to hang dynamic state off the plugin State parameter, to
/// be used between load->update->unload cycles. This adds the consequence that
/// any pointers to objects created from this type are invalidated when the
/// plugin is dropped.
///
/// Safety:
///
/// If this trait and macro are used, the plugin must be "unload"ed before being
/// dropped.
///
/// Arguments: (Plugin state struct, Plugin state argument type => state field)
#[macro_export]
macro_rules! impl_plugin {
    ($self_ty:ty, $assoc_type:ty => $field:ident) => {
        #[no_mangle]
        pub extern "C" fn load(state: &mut $assoc_type) {
            let mut this = <$self_ty>::new();
            this.load(state);
            state.$field = Some(this);
        }

        #[no_mangle]
        pub extern "C" fn update(state: &mut $assoc_type, delta_time: &std::time::Duration) {
            let mut this = state.$field.take();
            if let Some(mut this) = this {
                this.update(state, delta_time);
                state.$field = Some(this);
            }
        }

        #[no_mangle]
        pub extern "C" fn unload(state: &mut $assoc_type) {
            let mut this = state.$field.take();
            if let Some(mut this) = this {
                this.unload(state);
            }
        }
    };
}
