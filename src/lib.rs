//! Keeps a Vortex Studio project and a folder of plain files in sync, so the
//! project can live in git and be edited outside Studio.

pub mod config;
pub mod disk;
pub mod layout;
pub mod names;
pub mod ops;
pub mod serve;
pub mod state;
