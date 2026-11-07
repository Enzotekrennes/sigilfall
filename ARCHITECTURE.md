# Architecture

Sigilfall is a Cargo workspace with two crates.

| Crate | Kind | Responsibility |
|---|---|---|
| `crates/sigilfall-core` | library | Game logic: rules, world state, dungeon generation. |
| `crates/sigilfall` | binary | Terminal application: command line, input, screens, rendering. |

## Rules

- **`sigilfall-core` depends on no terminal crate.** No rendering, terminal or
  input library may appear in its dependency tree (`cargo tree -p sigilfall-core`).
  This keeps game logic testable without a terminal and makes the boundary
  between logic and display checked by the compiler.
- Dependencies only point from the binary to `sigilfall-core`, never the other way around.
