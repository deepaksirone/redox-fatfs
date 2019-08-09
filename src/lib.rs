#![crate_type="lib"]
#![allow(dead_code)]

#[macro_use]
extern crate log;

extern crate syscall;
extern crate spin;

extern crate byteorder;
#[macro_use]
extern crate bitflags;

use std::sync::atomic::AtomicUsize;
pub static IS_UMT: AtomicUsize = AtomicUsize::new(0);
pub type Result<T> = std::io::Result<T>;
pub const BLOCK_SIZE: u64 = 4096;
//pub use self::disk::{Disk, DiskCache, DiskFile};
pub use self::mount::mount;


mod bpb;
mod filesystem;
mod dir_entry;
mod table;
mod mount;

//pub use disk::*;
pub use bpb::*;
pub use filesystem::*;
pub use dir_entry::*;
pub use table::*;