//! Independent runtime foundations. Game bindings are a separate, unfinished layer.
//!
//! This crate contains no game address tables or guessed game object layouts.
#![deny(unsafe_op_in_unsafe_fn)]

pub mod callback;
pub mod code_patches;
pub mod error;
pub mod input;
pub mod memory;
pub mod native_tree;
pub mod pe;
pub mod profile;
pub mod rtti;
pub mod signature;
pub mod task_definitions;

pub use error::{Error, Result};
