## My toy project, which is:

- A 'game engine' written in Rust
- Meant as a playground and learning platform, building useful bits that I can use in other projects
- Uses the Vulkan graphics API (using `ash` crate for Vulkan bindings)
- Implements an OBJ model parser using nom
- Implements a common set of types shared with the shaders, which are themselves written in rust using `rust-gpu` and `spirv-std`.
- Implements a custom async executor for pinning to cpu and cpu-heavy loads
- An unsafe (and probably unsound) implementation of a scoped task to allow async tasks to borrow data mutably from their environment by relaxing the `'static` lifetime requirement.
- A stable type id based on the name of the type including its module path.
- UDP based networking, with message bitpacking and compression.

## Dead ideas:
- Runtime code loading, separation into plugins
TL;DR the rust ecosystem is not particularly ready for stable ABIs in the context of dynamically loaded code, and writing code that depends on a runtime loading scheme (as I have previously implemented here), simply resolves to more hoops than is worth jumping through. Any static state in a library is opaque and inaccessible to other loaded libraries. Re-implementing some other crates because they don't work well with dynamic loading isn't fun. If you're working on using dynamic objects in rust, my recommendation is to keep a dependency tree small and avoid static state.

## Current state:

As this is a toy project, it evolves in spurts and is not always in a working state. The current game shell implemented using the engine is a simple 3d view that can be controlled with the arrow keys. See the `nshell` bin target for more details.

![Current Version](/docs/images/current.gif)