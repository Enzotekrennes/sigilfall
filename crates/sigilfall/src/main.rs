//! Entry point of the Sigilfall terminal game.
//!
//! This binary owns everything tied to the terminal: command line, input,
//! screens and rendering. Game rules live in the `sigilfall-core` crate.

/// Prints the startup message with the game name and version.
fn main() {
    println!("Sigilfall {} (in development)", env!("CARGO_PKG_VERSION"));
}
