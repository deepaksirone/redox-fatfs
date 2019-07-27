use std::io;
use std::path::Path;
use std::io::{Read, Write, Seek};

use filesystem::FileSystem;

//#[cfg(target_os = "redox")]
mod redox;

/*
#[cfg(target_os = "redox")]
pub fn mount<D: Read + Write + Seek, P: AsRef<Path>, F: FnMut()>(filesystem: FileSystem<D>, mountpoint: &P, callback: F) -> io::Result<()> {
    redox::mount(filesystem, mountpoint, callback)
}
*/
