//! Core model for Linux Explorer: everything that does not draw pixels.
//!
//! The UI crate only renders state produced here, so all behaviour that has to
//! match Windows Explorer (ordering, grouping, naming, operations) is testable
//! without a display.

// clippy 1.95 panics inside this lint on our code (span slicing bug).
#![allow(clippy::cloned_ref_to_slice_refs)]

pub mod apps;
pub mod admin;
pub mod archive;
pub mod batch_rename;
pub mod clipboard;
pub mod devices;
pub mod entry;
pub mod filetype;
pub mod format;
pub mod git;
pub mod grouping;
pub mod history;
pub mod location;
pub mod logo;
pub mod meta;
pub mod names;
pub mod natsort;
pub mod ops;
pub mod places;
pub mod props;
pub mod search;
pub mod servicemenus;
pub mod settings;
pub mod sorting;
pub mod terminal;
pub mod trashbin;
pub mod undo;

pub use entry::{Entry, EntryKind};
pub use location::Location;
