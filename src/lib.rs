#![crate_type="lib"]

//#[macro_use]
//extern crate log;

extern crate syscall;
extern crate byteorder;
#[macro_use]
extern crate bitflags;


pub type Result<T> = std::io::Result<T>;
pub const BLOCK_SIZE: u64 = 4096;
//pub use self::disk::{Disk, DiskCache, DiskFile};



mod disk;
mod bpb;
mod filesystem;
mod file;
mod table;

pub use disk::*;
pub use bpb::*;
pub use filesystem::*;
pub use file::*;
pub use table::*;