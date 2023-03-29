#[cfg(test)]
mod tests {

    use plugin_loader::Plugin;

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

        let mut state = ();

        plugin.check(&mut state).unwrap();
        plugin
            .call_update(&mut state, &std::time::Duration::from_secs(1))
            .await
            .unwrap();
        drop(plugin);
    }
}
