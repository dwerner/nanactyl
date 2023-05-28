use logger::{info, LogLevel, Logger};
use plugin_self::{impl_plugin_static, PluginState};
use world::World;

struct AbcPlugin {
    plugin_state: u32,
    logger: Logger,
}

const NAME: &str = "abc-plugin";
const _VERSION: u64 = 0;

impl PluginState for AbcPlugin {
    type GameState = World;

    fn new() -> Box<Self>
    where
        Self: Sized,
    {
        Box::new(AbcPlugin {
            plugin_state: 42,
            logger: LogLevel::Info.logger().sub("abc-plugin"),
        })
    }

    fn load(&mut self, _state: &mut Self::GameState) {
        info!(
            self.logger.sub("load"),
            "{} loaded state {}", NAME, self.plugin_state
        );
    }

    fn update(&mut self, _state: &mut Self::GameState, _delta_time: &std::time::Duration) {
        info!(
            self.logger.sub("update"),
            "{} updated state {}", NAME, self.plugin_state
        );
    }

    fn unload(&mut self, _state: &mut Self::GameState) {
        info!(
            self.logger.sub("unload"),
            "{} unloaded {}", NAME, self.plugin_state
        );
    }
}

impl_plugin_static!(AbcPlugin, World);
