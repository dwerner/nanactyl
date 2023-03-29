use plugin_trait::{impl_plugin, LoadablePlugin};

struct AbcPlugin;

impl LoadablePlugin for AbcPlugin {
    const NAME: &'static str = "abc_plugin";
    const VERSION: u64 = 1;

    type State = ();

    fn load(_state: &mut Self::State) {
        println!("{} loaded", Self::NAME);
    }

    fn update(_state: &mut Self::State, _delta_time: &std::time::Duration) {
        println!("{} updated", Self::NAME);
    }

    fn unload(_state: &mut Self::State) {
        println!("{} unloaded", Self::NAME);
    }
}

impl_plugin!(AbcPlugin, ());
