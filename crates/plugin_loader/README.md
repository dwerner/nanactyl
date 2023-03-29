# Plugin System Overview

This plugin system enables the game engine to dynamically load, unload, and update plugins at runtime. The primary purpose of this system is to extend the game engine's functionality through the use of external modules without requiring a full recompilation of the engine. 

The `Plugin` struct is the main component of the plugin system, managing the lifecycle of a single plugin. It is responsible for loading and unloading plugins, as well as invoking their lifecycle methods (load, unload, and update).

## Features

1. **Dynamic plugin loading and unloading**: Plugins can be loaded and unloaded during runtime without stopping the engine. This allows developers to easily extend or modify the game engine's functionality without recompiling the entire engine.

2. **Lifecycle methods**: Each plugin is expected to implement three lifecycle methods: `load`, `unload`, and `update`. These methods are called at specific points during the plugin's lifecycle to ensure proper integration with the game engine.

3. **Automatic plugin updates**: The plugin system checks for updates to plugins based on a specified interval. If a newer version of a plugin is detected, the system unloads the old version and loads the new version, ensuring that the game engine always uses the most up-to-date functionality.

4. **Thread-safe**: The plugin system is designed to be thread-safe, allowing multiple threads to interact with the system without causing issues.

## Usage

1. **Creating a new plugin**: Create a new plugin by implementing the required lifecycle methods (`load`, `unload`, and `update`) in your preferred programming language. Compile your plugin into a shared library (`.so` on Unix systems or `.dll` on Windows systems).

2. **Loading a plugin**: Use the `Plugin::open_from_target_dir` or `Plugin::open_at` methods to load a plugin from a specified path or target directory. The `Plugin::check` method must be called subsequently to load the plugin and invoke the `load` lifecycle method.

3. **Updating a plugin**: Call the `Plugin::call_update` method to invoke the `update` lifecycle method of the loaded plugin. This method should be called periodically to ensure the plugin's functionality remains up-to-date.

4. **Unloading a plugin**: Unload a plugin by calling the `Plugin::call_unload` method. This will invoke the `unload` lifecycle method and release any resources associated with the plugin.

## Example

```rust
let spawner = ThreadAffineSpawner::default();
let plugin_dir = "path/to/plugin_directory";
let plugin_name = "my_plugin";

let mut plugin = Plugin::open_from_target_dir(spawner, plugin_dir, plugin_name)?;
let mut state = MyGameState::new();

plugin.check(&mut state)?;

plugin.call_update(&mut state, &Duration::from_millis(16)).await?;
```

This example demonstrates how to create a new Plugin instance, load a plugin from a target directory, and call the update lifecycle method. Remember to periodically call the Plugin::call_update method to ensure the plugin's functionality remains up-to-date.