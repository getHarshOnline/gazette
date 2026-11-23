//! Shared values and invariants that every Gazette crate builds on.
//!
//! This crate builds on no other Gazette crate. `gazette-ports`, the compiler,
//! the application, every adapter and the SDK depend on it. [`source`] describes
//! retained inputs, [`publication`] their resolved public projections, and
//! [`release`] the evidence required to authorize delivery. [`effect`] keeps
//! permissions explicit. These are values only: constructing or validating them
//! acquires no source, renders no site and performs no release.

pub mod destination;
pub mod documentation;
pub mod effect;
pub mod font;
pub mod hosting;
pub mod http;
pub mod json;
pub mod markdown;
pub mod mathml;
pub mod plugin;
pub mod preview;
pub mod process;
pub mod publication;
pub mod raster;
pub mod release;
pub mod render;
pub mod resource;
pub mod source;
pub mod stage;
pub mod store;
