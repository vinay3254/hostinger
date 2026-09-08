pub mod api;
pub mod builder;
pub mod config;
pub mod detector;
pub mod model;
pub mod runtime;
pub mod service;
pub mod store;

pub type Result<T> = anyhow::Result<T>;
