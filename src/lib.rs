#![crate_type="lib"]

extern crate syscall;
extern crate byteorder;

pub type Result<T> = std::io::Result<T>;
pub const BLOCK_SIZE: u64 = 4096;
//pub use self::disk::{Disk, DiskCache, DiskFile};
pub use self::bpb::{BiosParameterBlock, BiosParameterBlockLegacy, BiosParameterBlockFAT32, FATType};

mod disk;
mod bpb;
mod filesystem;
