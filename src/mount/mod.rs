use std::io;
use std::path::Path;
use std::io::{Read, Write, Seek};

use filesystem::FileSystem;

//#[cfg(target_os = "redox")]
mod redox;


//#[cfg(target_os = "redox")]
pub fn mount<D: Read + Write + Seek, P: AsRef<Path>, F: FnMut()>(filesystem: FileSystem<D>, mountpoint: &P, callback: F, mount_mode: u16, mount_uid: u32, mount_gid: u32) -> io::Result<()> {
    redox::mount(filesystem, mountpoint, callback, mount_uid, mount_gid, mount_mode)
}

