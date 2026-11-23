//! Turns authored content and a workspace's configuration into the one resolved
//! publication model that every output reads.
//!
//! This crate builds on `gazette-core` and `gazette-ports`; `gazette-application` and
//! `gazette-sdk` build on it. It is pure compilation, with no filesystem, database,
//! network or interface effects. Start with [`workspace`] for configuration validation
//! and resolved ownership, then [`corpus`] and [`authored`] for supplied content and
//! its page projections. [`theme`] and [`theme_package`] describe presentation
//! contracts; native resource acquisition remains outside this crate. A valid model
//! establishes structural consistency, not publication permission, deployment, or an
//! observed outcome.

pub mod authored;
pub mod contracts;
pub mod corpus;
pub mod css;
pub mod disclosure;
pub mod document;
pub mod encoded;
pub mod generated_mdx;
pub mod html;
pub mod javascript;
pub mod landing;
pub mod markdown;
pub mod notebook;
pub mod plugin;
pub mod release;
mod schema;
mod tables;
pub mod theme;
pub mod theme_graph;
pub mod theme_package;
pub mod toml;
pub mod workspace;
pub mod xml;
pub mod yaml;
