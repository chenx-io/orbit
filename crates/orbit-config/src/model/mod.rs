//! Config model layer - pure serde data definitions (no parsing/interpolation logic)
//!
//! Type re-exports are done at the crate root (`lib.rs`) to keep the public API stable.

pub mod action;
pub mod check;
pub mod datasource;
pub mod extract;
pub mod plan;
pub mod request;
pub mod step;
pub mod template;
