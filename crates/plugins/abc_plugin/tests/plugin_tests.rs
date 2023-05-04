#[cfg(test)]
mod tests {

    use logger::{info, LogLevel};
    use plugin_loader::Plugin;
    use world::World;

    #[smol_potat::test]
    async fn loading_abc_plugin() {
        let mut root = std::env::current_dir().unwrap();
        root.pop();
        root.pop();
        root.pop();

        duct::cmd!(
            "cargo",
            "build",
            "--manifest-path",
            "crates/plugins/abc_plugin/Cargo.toml"
        )
        .dir(&root)
        .run()
        .unwrap();

        let target_dir = root.join("target/debug");

        let mut plugin = Plugin::open_from_target_dir(&target_dir, "abc_plugin").unwrap();

        let logger = LogLevel::Debug.logger().sub("test");

        // World is special, it holds on to state that is set by the plugin.
        let mut state = World::new(None, &logger, true);

        plugin.check(&mut state).unwrap();
        plugin
            .call_update(&mut state, &std::time::Duration::from_secs(1))
            .await
            .unwrap();

        info!(logger, "rebuilding plugin after clean");

        duct::cmd!(
            "cargo",
            "clean",
            "--manifest-path",
            "crates/plugins/abc_plugin/Cargo.toml",
        )
        .dir(&root)
        .run()
        .unwrap();

        duct::cmd!(
            "cargo",
            "build",
            "--manifest-path",
            "crates/plugins/abc_plugin/Cargo.toml"
        )
        .dir(&root)
        .run()
        .unwrap();

        plugin.check(&mut state).unwrap();
        plugin
            .call_update(&mut state, &std::time::Duration::from_secs(1))
            .await
            .unwrap();

        plugin.call_unload(&mut state).unwrap();
        drop(plugin);
    }
}
