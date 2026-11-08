//! Game logic of Sigilfall: rules, world state and dungeon generation.
//!
//! This crate is pure logic. It never depends on a terminal, rendering or input
//! library, so every rule can be tested without a terminal. The `sigilfall`
//! binary turns player input into actions for this crate and draws the
//! resulting state.

pub mod action;
pub mod actor;
pub mod ai;
pub mod content;
pub mod event;
pub mod fov;
pub mod game;
pub mod generation;
pub mod geometry;
pub mod map;
pub mod path;
pub mod rng;
pub mod schedule;
