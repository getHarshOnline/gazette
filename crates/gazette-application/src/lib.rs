//! The operations that turn a compiled publication into results: loading a workspace,
//! authoring analysis, building, previewing, keeping the release store and releasing.
//!
//! This crate builds on `gazette-compiler`, `gazette-core` and `gazette-ports`;
//! `gazette-sdk` builds on it and supplies the native adapters. It owns no concrete
//! filesystem, database, renderer or interface. [`configuration`] loads an explicitly
//! selected workspace, [`authoring`] analyses supplied sources, and [`build`] composes
//! publication output through caller-provided ports. [`preview`], [`store`] and
//! [`release`] retain their separate effect and evidence boundaries; success in one is
//! not authority for another.

pub mod authoring;
pub mod build;
pub mod configuration;
pub mod delivery;
pub mod facts;
pub mod preview;
pub mod release;
pub mod source;
pub mod store;
