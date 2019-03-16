#![crate_type="lib"]

extern crate syscall;

pub use self::disk::{Disk, DiskCache, DiskFile};
pub use self::bpb::{BiosParameterBlock, BiosParameterBlockLegacy, BiosParameterBlockFAT32, FATType};
pub const BLOCK_SIZE: u64 = 4096;

mod disk;
mod bpb;
mod filesystem;
