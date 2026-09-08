pub mod api;
pub mod auth;
pub mod builder;
pub mod config;
pub mod db;
pub mod detector;
pub mod model;
pub mod repository;
pub mod runtime;
pub mod service;
pub mod store;

pub type Result<T> = anyhow::Result<T>;
