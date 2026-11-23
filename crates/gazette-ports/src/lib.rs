//! The capabilities Gazette's operations ask the outside world for, as traits: reading
//! sources, writing output, running tools, reading Git and the like.
//!
//! This crate builds on `gazette-core`; `gazette-compiler`, `gazette-application`, every
//! adapter and `gazette-sdk` build on it. The adapters implement these traits, and the
//! SDK chooses which implementation to use. Traits describe exactly what an application
//! operation may ask an adapter to do. Each operation documents its evidence, limits
//! and failure contract; a trait implementation is not itself permission to invoke it.
//! Domain values remain in `gazette-core`.

pub mod capability;
pub mod destination;
pub mod documentation;
pub mod facts;
pub mod font;
pub mod hosting;
pub mod http;
pub mod markdown;
pub mod preview;
pub mod process;
pub mod raster;
pub mod release;
pub mod render;
pub mod resource;
pub mod source;
pub mod stage;
pub mod store;
