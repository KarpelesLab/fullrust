//! Convenient glob import for fullrust programs.
//!
//! ```ignore
//! use fullrust::prelude::*;
//! ```
//!
//! Brings in the print macros, the `#[main]` attribute, the common `alloc`
//! types, and the `env`/`io`/`rt` modules.

pub use crate::entry;
pub use crate::{eprint, eprintln, print, println};

pub use crate::{env, io, rt};

pub use alloc::borrow::ToOwned;
pub use alloc::boxed::Box;
pub use alloc::string::{String, ToString};
pub use alloc::vec::Vec;
pub use alloc::{format, vec};
