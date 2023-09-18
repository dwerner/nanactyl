
# `nshell` - Game Engine Binary

`nshell` is a binary that composes different parts of the game engine into a client and server that can connect together, sync state, and render using the ash/vulkan rendering plugin. It manages the main loop, event handling, and plugin updates for the game engine.

## Features

- Initializes and manages the game world, platform context, plugins, and render state.
- Handles input events and updates input state.
- Loads and updates plugins, including the renderer, world updater, net sync, asset loader, and world-render state updater.
- Runs the main loop (`frame_loop`) for processing events, updating plugins, and rendering the game.
- `nshell` utilizes the `core_executor` crate, which is a lightweight executor for futures, to manage concurrent tasks within the game engine. It creates a `ThreadPoolExecutor` with a specified number of threads, in this case 8, and uses the executor's spawners to handle tasks concurrently. This allows `nshell` to efficiently manage tasks such as plugin updates, asset loading, world updates, and rendering, which are essential for the game engine's performance and responsiveness.


## Usage

1. Compile the `nshell` binary from source.
2. Run the `nshell` binary with the appropriate command-line options.

### Command-Line Options

- `--cwd`: Optional path to change the current working directory.
- `--backtrace`: Enable/disable stack traces (default: false).
- `--enable_validation_layer`: Enable/disable the Vulkan validation layer (default: false).
- `--connect_to_server`: Optional address to connect to a game server.

## Example use

To run the `nshell` binary as a client and connect to a game server at `192.168.1.100:12345`, use the following command:

```bash
nshell --connect_to_server 192.168.1.100:12345
```