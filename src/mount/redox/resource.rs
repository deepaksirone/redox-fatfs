use std::cmp::{min, max};
use std::time::{SystemTime, UNIX_EPOCH};
use std::io::{Read, Write, Seek};

use syscall::data::{Map, Stat, TimeSpec};
use syscall::error::{Error, Result, EBADF, EBUSY, EINVAL, EISDIR, EPERM};
use syscall::flag::{O_ACCMODE, O_RDONLY, O_WRONLY, O_RDWR, F_GETFL, F_SETFL, MODE_PERM, PROT_READ, PROT_WRITE, SEEK_SET, SEEK_CUR, SEEK_END};

use filesystem::FileSystem;
use dir_entry::{Dir, File};

use super::scheme::{Fmaps, FmapKey, FmapValue};


pub trait Resource<D: Read + Write + Seek> {
    fn start_cluster(&self) -> u64;
    fn dup(&self) -> Result<Box<dyn Resource<D>>>;
    fn read(&mut self, buf: &mut [u8], fs: &mut FileSystem<D>) -> Result<usize>;
    fn write(&mut self, buf: &[u8], fs: &mut FileSystem<D>) -> Result<usize>;
    fn seek(&mut self, offset: usize, whence: usize, fs: &mut FileSystem<D>) -> Result<usize>;
    fn fmap(&mut self, map: &Map, maps: &mut Fmaps, fs: &mut FileSystem<D>) -> Result<usize>;
    fn funmap(&mut self, maps: &mut Fmaps, fs: &mut FileSystem<D>) -> Result<usize>;
    fn fchmod(&mut self, mode: u16, fs: &mut FileSystem<D>) -> Result<usize>;
    fn fchown(&mut self, uid: u32, gid: u32, fs: &mut FileSystem<D>) -> Result<usize>;
    fn fcntl(&mut self, cmd: usize, arg: usize) -> Result<usize>;
    fn path(&self, buf: &mut [u8]) -> Result<usize>;
    fn stat(&self, _stat: &mut Stat, fs: &mut FileSystem<D>) -> Result<usize>;
    fn sync(&mut self, maps: &mut Fmaps, fs: &mut FileSystem<D>) -> Result<usize>;
    fn truncate(&mut self, len: usize, fs: &mut FileSystem<D>) -> Result<usize>;
    fn utimens(&mut self, times: &[TimeSpec], fs: &mut FileSystem<D>) -> Result<usize>;
}

pub struct DirResource {
    dir: Dir,
    data: Option<Vec<u8>>,
    seek: usize,
    uid: u32,
}

impl DirResource {
    pub fn new(dir: Dir, data: Option<Vec<u8>>, uid: u32) -> DirResource {
        DirResource {
            dir: dir,
            data: data,
            seek: 0,
            uid: uid,
        }
    }
}

impl<D: Read + Write + Seek> Resource<D> for DirResource {
    fn start_cluster(&self) -> u64 {
        self.dir.first_cluster.cluster_number
    }

    fn dup(&self) -> Result<Box<dyn Resource<D>>> {
        Ok(Box::new(
           DirResource {
               dir: self.dir.clone(),
               data: self.data.clone(),
               seek: self.seek,
               uid: self.uid
           }
        ))
    }

    fn read(&mut self, buf: &mut [u8], fs: &mut FileSystem<D>) -> Result<usize> {
        let data = self.data.as_ref().ok_or(Error::new(EISDIR))?;
        let mut i = 0;
        while i < buf.len() && self.seek < data.len() {
            buf[i] = data[self.seek];
            i += 1;
            self.seek += 1;
        }
        Ok(i)
    }

    fn write(&mut self, _buf: &[u8], _fs: &mut FileSystem<D>) -> Result<usize> {
        Err(Error::new(EBADF))
    }


    fn seek(&mut self, offset: usize, whence: usize, _fs: &mut FileSystem<D>) -> Result<usize> {
        let data = self.data.as_ref().ok_or(Error::new(EBADF))?;
        self.seek = match whence {
            SEEK_SET => max(0, min(data.len() as isize, offset as isize)) as usize,
            SEEK_CUR => max(0, min(data.len() as isize, self.seek as isize + offset as isize)) as usize,
            SEEK_END => max(0, min(data.len() as isize, data.len() as isize + offset as isize)) as usize,
            _ => return Err(Error::new(EINVAL))
        };

        Ok(self.seek)
    }

    fn fmap(&mut self, _map: &Map, _maps: &mut Fmaps, _fs: &mut FileSystem<D>) -> Result<usize> {
        Err(Error::new(EBADF))
    }
    fn funmap(&mut self, _maps: &mut Fmaps, _fs: &mut FileSystem<D>) -> Result<usize> {
        Err(Error::new(EBADF))
    }

    fn fchmod(&mut self, mode: u16, fs: &mut FileSystem<D>) -> Result<usize> {
        Ok(0) //No notion of permissions in FAT
    }

    fn fchown(&mut self, uid: u32, gid: u32, fs: &mut FileSystem<D>) -> Result<usize> {
        Ok(0)
    }

    fn fcntl(&mut self, _cmd: usize, _arg: usize) -> Result<usize> {
        Err(Error::new(EBADF))
    }

    fn path(&self, buf: &mut [u8]) -> Result<usize> {
        let path = self.dir.dir_path.as_bytes();

        let mut i = 0;
        while i < buf.len() && i < path.len() {
            buf[i] = path[i];
            i += 1;
        }

        Ok(i)
    }

    fn stat(&self, stat: &mut Stat, fs: &mut FileSystem<D>) -> Result<usize> {


        *stat = Stat {
            st_dev: 0, // TODO
            st_ino: self.dir.first_cluster.cluster_number,
            st_mode: 0o755, //TODO
            st_nlink: 1,
            st_uid: 0,
            st_gid: 0,
            st_size: self.dir.size(fs),
            st_mtime: 0, //TODO
            st_mtime_nsec: 0,
            st_ctime: 0,
            st_ctime_nsec: 0,
            ..Default::default()
        };

        Ok(0)
    }

    fn sync(&mut self, _maps: &mut Fmaps, _fs: &mut FileSystem<D>) -> Result<usize> {
        Err(Error::new(EBADF))
    }

    fn truncate(&mut self, _len: usize, _fs: &mut FileSystem<D>) -> Result<usize> {
        Err(Error::new(EBADF))
    }

    fn utimens(&mut self, _times: &[TimeSpec], _fs: &mut FileSystem<D>) -> Result<usize> {
        Err(Error::new(EBADF))
    }

}

pub struct FileResource {
    file: File,
    flags: usize,
    seek: u64,
    uid: u32,
    fmap: Option<(usize, FmapKey)>
}
/*
impl<D: Read + Write + Seek> Resource<D> for FileResource {
    fn start_cluster(&self) -> u64 {
        self.file.first_cluster.cluster_number
    }

    fn dup(&self) -> Result<Box<Resource<D>>> {
        Ok(Box::new(
            FileResource {
                file: self.file.clone(),
                flags: self.flags,
                seek: self.seek,
                uid: self.uid,
                fmap: None
            }
        ))
    }

    fn read(&mut self, buf: &mut [u8], fs: &mut FileSystem<D>) -> Result<usize> {

    }
}*/