extern crate spin;
use syscall;
use syscall::{Packet, Scheme};
use std::fs::File;
use std::io::{self, Read, Write, Seek};
use std::path::Path;
use std::sync::atomic::Ordering;

use IS_UMT;
use filesystem::FileSystem;
use self::scheme::FileScheme;

pub mod resource;
pub mod scheme;
pub mod result;

pub fn mount<D: Read + Write + Seek, P: AsRef<Path>, F: FnMut()>(filesystem: FileSystem<D>, mountpoint: &P, mut callback: F
                    ,mount_uid: u32, mount_gid: u32, mount_mode: u16) -> io::Result<()> {
    let mountpoint = mountpoint.as_ref();
    let mut socket = File::create(format!(":{}", mountpoint.display()))?;

    callback();

    syscall::setrens(0, 0).expect("redox-fatfs: failed to enter null namespace");

    let scheme = FileScheme::new(format!("{}", mountpoint.display()), filesystem,
                                mount_mode, mount_uid, mount_gid);
    loop {
        if IS_UMT.load(Ordering::SeqCst) > 0 {
            break Ok(());
        }

        let mut packet = Packet::default();
        match socket.read(&mut packet) {
            Ok(_ok) => (),
            Err(err) => if err.kind() == io::ErrorKind::Interrupted {
                continue;
            } else {
                break Err(err);
            }
        }

        scheme.handle(&mut packet);

        match socket.write(&packet) {
            Ok(_ok) => (),
            Err(err) => {
                break Err(err);
            }
        }
    }
}
