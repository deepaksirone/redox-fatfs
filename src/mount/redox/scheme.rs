//Modified from redoxfs/mount/redox/scheme.rs

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::str;
use std::sync::atomic::{AtomicUsize, Ordering};
//use std::time::{SystemTime, UNIX_EPOCH};
use std::io::{Read, Write, Seek};

use syscall::data::{Map, Stat, StatVfs, TimeSpec};
use syscall::error::{Error, Result, EACCES, EEXIST, EISDIR, ENOTDIR, EPERM, ENOENT, EBADF, EINVAL};
use syscall::flag::{O_APPEND, O_CREAT, O_DIRECTORY, O_EXCL, O_TRUNC, O_ACCMODE, O_RDONLY, O_WRONLY, O_RDWR, O_SYMLINK};
use syscall::scheme::Scheme;


use filesystem::FileSystem;
use dir_entry::Dir;
use table::get_free_count;

use super::result::from;
use super::resource::{Resource, DirResource, FileResource};
use super::spin::Mutex;

const FMAP_AMOUNT: usize = 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FmapKey {
    pub block: u64,
    pub offset: usize,
    pub size: usize
}

#[derive(Clone)]
pub struct FmapValue {
    pub buffer: Vec<u8>,
    /// The actual file length. Syncing only writes &buffer[..actual_size].
    pub actual_size: usize,
    pub refcount: usize
}

const MODE_WRITE: u16 = 0o2;
const MODE_READ: u16 = 0o4;

pub struct Fmaps(Vec<Option<(FmapKey, FmapValue)>>);
impl Default for Fmaps {
    fn default() -> Fmaps {
        Fmaps(vec![None; FMAP_AMOUNT])
    }
}

pub struct FileScheme<D: Read + Write + Seek> {
    name: String,
    fs: RefCell<FileSystem<D>>,
    next_id: AtomicUsize,
    files: Mutex<BTreeMap<usize, Box<dyn Resource<D>>>>,
    fmaps: Mutex<Fmaps>,
    mount_mode: u16,
    mount_uid: u32,
    mount_gid: u32
}

//Move the permission checking to the scheme
//FAT does not have provision for permissions
impl<D: Read + Write + Seek> FileScheme<D> {
    fn permission(&self, uid: u32, gid: u32, op: u16) -> bool {
        let mut perm = self.mount_mode & 0o7;
        if self.mount_uid == uid {
            // If self.mode is 101100110, >> 6 would be 000000101
            // 0o7 is octal for 111, or, when expanded to 9 digits is 000000111
            perm |= (self.mount_mode >> 6) & 0o7;
            // Since we erased the GID and OTHER bits when >>6'ing, |= will keep those bits in place.
        }
        if self.mount_gid == gid || gid == 0 {
            perm |= (self.mount_mode >> 3) & 0o7;
        }
        if uid == 0 {
            //set the `other` bits to 111
            perm |= 0o7;
        }
        perm & op == op
    }

    fn owner(&self, uid: u32) -> bool {
        uid == 0 || self.mount_uid == uid
    }

    pub fn new(name: String, fs: FileSystem<D>, mount_mode: u16, mount_uid: u32, mount_gid: u32) -> FileScheme<D> {
        FileScheme {
            name: name,
            fs: RefCell::new(fs),
            next_id: AtomicUsize::new(1),
            files: Mutex::new(BTreeMap::new()),
            fmaps: Mutex::new(Fmaps::default()),
            mount_mode: mount_mode,
            mount_uid: mount_uid,
            mount_gid: mount_gid
        }
    }
}

impl<D: Read + Write + Seek> Scheme for FileScheme<D> {
    fn open(&self, url: &[u8], flags: usize, uid: u32, gid: u32) -> Result<usize> {
        let path = str::from_utf8(url).unwrap_or("").trim_matches('/');

        println!("Open '{}' {:X}", path, flags);

        let mut fs = self.fs.borrow_mut();
        let dentry = Dir::get_entry_abs(path, &mut fs).ok();
        println!("Found dir entry for path = {:?}", path);
        //let node_opt = self.path_nodes(&mut fs, path, uid, gid, &mut nodes)?;
        let resource: Box<dyn Resource<D>> = match dentry {
            Some(e) => if flags & (O_CREAT | O_EXCL) == O_CREAT | O_EXCL {
                return Err(Error::new(EEXIST));
            } else if e.is_dir() {
                if flags & O_ACCMODE == O_RDONLY {
                    if !self.permission(uid, gid, MODE_READ) {
                        // println!("dir not readable {:o}", node.1.mode);
                        return Err(Error::new(EACCES));
                    }

                    //let mut children = Vec::new();
                    //fs.child_nodes(&mut children, node.0)?;

                    let mut data = Vec::new();
                    for child in e.to_dir().to_iter(&mut fs) {
                        let name = child.name();
                        if !data.is_empty() {
                                data.push(b'\n');
                        }
                        data.extend_from_slice(&name.as_bytes());
                    }
                    println!("Created a dirResource for path = {:?} with data = {:?}", path, data);
                    Box::new(DirResource::new(e.to_dir(), Some(data), Some(self.mount_uid),
                                              Some(self.mount_gid), Some(self.mount_mode)))
                } else if flags & O_WRONLY == O_WRONLY {
                    // println!("{:X} & {:X}: EISDIR {}", flags, O_DIRECTORY, path);
                    return Err(Error::new(EISDIR));
                } else {
                    Box::new(DirResource::new(e.to_dir(), None, Some(self.mount_uid),
                                              Some(self.mount_gid), Some(self.mount_mode)))
                }
            } /*else if node.1.is_symlink() && !(flags & O_STAT == O_STAT && flags & O_NOFOLLOW == O_NOFOLLOW) && flags & O_SYMLINK != O_SYMLINK {
                let mut resolve_nodes = Vec::new();
                let resolved = self.resolve_symlink(&mut fs, uid, gid, url, node, &mut resolve_nodes)?;
                drop(fs);
                return self.open(&resolved, flags, uid, gid);
            }*/
              else if flags & O_SYMLINK == O_SYMLINK {
                return Err(Error::new(EINVAL));
            } else {
                if flags & O_DIRECTORY == O_DIRECTORY {
                    // println!("{:X} & {:X}: ENOTDIR {}", flags, O_DIRECTORY, path);
                    return Err(Error::new(ENOTDIR));
                }

                if (flags & O_ACCMODE == O_RDONLY || flags & O_ACCMODE == O_RDWR) && !self.permission(uid, gid, MODE_READ) {
                    // println!("file not readable {:o}", node.1.mode);
                    return Err(Error::new(EACCES));
                }

                if (flags & O_ACCMODE == O_WRONLY || flags & O_ACCMODE == O_RDWR) && !self.permission(uid, gid, MODE_WRITE) {
                    // println!("file not writable {:o}", node.1.mode);
                    return Err(Error::new(EACCES));
                }

                if flags & O_TRUNC == O_TRUNC {
                    if self.permission(uid, gid, MODE_WRITE) {
                        // println!("file not writable {:o}", node.1.mode);
                        return Err(Error::new(EACCES));
                    }

                    from(e.to_file().truncate(&mut fs, 0))?;
                }

                let seek = if flags & O_APPEND == O_APPEND {
                    e.to_file().size()
                } else {
                    0
                };

                Box::new(FileResource::new(e.to_file(), flags,
                                           seek, Some(self.mount_uid), Some(self.mount_gid), Some(self.mount_mode)))
            },
            None => if flags & O_CREAT == O_CREAT {
                let mut last_part = String::new();
                for part in path.split('/') {
                    if !part.is_empty() {
                        last_part = part.to_string();
                    }
                }

                if last_part.is_empty() {
                    return Err(Error::new(EPERM))
                }
                //let parent_dir = from(Dir::get_parent(path, fs))?

                let root_dir = fs.root_dir();

                // Mount point root '/' may be accessed if permissions match
                if self.permission(uid, gid, MODE_WRITE) {
                    // println!("dir not writable {:o}", parent.1.mode);
                    return Err(Error::new(EACCES));
                }

                let dir = flags & O_DIRECTORY == O_DIRECTORY;

                if flags & O_SYMLINK == O_SYMLINK {
                    return Err(Error::new(EPERM))
                }

                        /*let ctime = SystemTime::now().duration_since(UNIX_EPOCH).unwrap();
                        let mut node = fs.create_node(mode_type | (flags as u16 & MODE_PERM), &last_part, parent.0, ctime.as_secs(), ctime.subsec_nanos())?;
                        node.1.uid = uid;
                        node.1.gid = gid;
                        fs.write_at(node.0, &node.1)?;*/


                if dir {
                    let d = from(root_dir.create_dir(path, &mut fs))?;
                    Box::new(DirResource::new(d, None,
                                              Some(self.mount_uid), Some(self.mount_gid),Some(self.mount_mode)))
                } else {
                    let file = from(root_dir.create_file(path, &mut fs))?;
                    let seek = if flags & O_APPEND == O_APPEND {
                        file.size()
                    } else {
                        0
                    };

                    Box::new(FileResource::new(file, flags, seek, Some(self.mount_uid), Some(self.mount_gid), Some(self.mount_mode)))
                }


            } else {
                return Err(Error::new(ENOENT));
            }
        };

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        self.files.lock().insert(id, resource);

        Ok(id)
    }

    fn chmod(&self, _url: &[u8], _mode: u16, _uid: u32, _gid: u32) -> Result<usize> {
        Ok(0)
    }

    fn rmdir(&self, url: &[u8], uid: u32, gid: u32) -> Result<usize> {
        let path = str::from_utf8(url).unwrap_or("").trim_matches('/');

        // println!("Rmdir '{}'", path);

        let mut fs = self.fs.borrow_mut();

        let dir_ent = Dir::get_entry_abs(path, &mut fs).ok();
        if let Some(child) = dir_ent {
            if ! self.permission(uid, gid, MODE_WRITE) {
                // println!("dir not writable {:o}", parent.1.mode);
                    return Err(Error::new(EACCES));
            }

            if child.is_dir() {
                let root_dir = fs.root_dir();
                from(root_dir.remove(path, &mut fs).map(|_x| 0 as usize))
            } else {
                    Err(Error::new(ENOTDIR))
            }

        } else {
            Err(Error::new(ENOENT))
        }
    }

    fn unlink(&self, url: &[u8], uid: u32, gid: u32) -> Result<usize> {
        let path = str::from_utf8(url).unwrap_or("").trim_matches('/');

        // println!("Unlink '{}'", path);

        let mut fs = self.fs.borrow_mut();


        if let Some(child) = Dir::get_entry_abs(path, &mut fs).ok() {
                if ! self.permission(uid, gid, MODE_WRITE) {
                    // println!("dir not writable {:o}", parent.1.mode);
                    return Err(Error::new(EACCES));
                }

                if ! child.is_dir() {
                    let root_dir = fs.root_dir();
                    from(root_dir.remove(path, &mut fs).map(|_r| 0 as usize))
                } else {
                    Err(Error::new(EISDIR))
                }

        } else {
            Err(Error::new(ENOENT))
        }
    }

    /* Resource operations */
    #[allow(unused_variables)]
    fn dup(&self, old_id: usize, buf: &[u8]) -> Result<usize> {
        // println!("Dup {}", old_id);

        if ! buf.is_empty() {
            return Err(Error::new(EINVAL));
        }

        let mut files = self.files.lock();
        let resource = if let Some(old_resource) = files.get(&old_id) {
            old_resource.dup()?
        } else {
            return Err(Error::new(EBADF));
        };

        let id = self.next_id.fetch_add(1, Ordering::SeqCst);
        files.insert(id, resource);

        Ok(id)
    }

    #[allow(unused_variables)]
    fn read(&self, id: usize, buf: &mut [u8]) -> Result<usize> {
        println!("Read {}, {:X} {}", id, buf.as_ptr() as usize, buf.len());
        let mut files = self.files.lock();
        let mut fs = self.fs.borrow_mut();
        if let Some(file) = files.get_mut(&id) {
            file.read(buf, &mut fs)
        } else {
            Err(Error::new(EBADF))
        }
    }

    fn write(&self, id: usize, buf: &[u8]) -> Result<usize> {
        println!("Write {}, {:X} {}", id, buf.as_ptr() as usize, buf.len());
        let mut files = self.files.lock();
        let mut fs = self.fs.borrow_mut();
        if let Some(file) = files.get_mut(&id) {
            file.write(buf, &mut fs)
        } else {
            Err(Error::new(EBADF))
        }
    }

    fn seek(&self, id: usize, pos: usize, whence: usize) -> Result<usize> {
        println!("Seek {}, {} {}", id, pos, whence);
        let mut files = self.files.lock();
        let mut fs = self.fs.borrow_mut();
        if let Some(file) = files.get_mut(&id) {
            file.seek(pos, whence, &mut fs)
        } else {
            Err(Error::new(EBADF))
        }
    }

    fn fchmod(&self, _id: usize, _mode: u16) -> Result<usize> {
        Ok(0)
    }

    fn fchown(&self, _id: usize, _uid: u32, _gid: u32) -> Result<usize> {
        Ok(0)
    }

    fn fcntl(&self, id: usize, cmd: usize, arg: usize) -> Result<usize> {
        let mut files = self.files.lock();
        if let Some(file) = files.get_mut(&id) {
            file.fcntl(cmd, arg)
        } else {
            Err(Error::new(EBADF))
        }
    }

    fn fpath(&self, id: usize, buf: &mut [u8]) -> Result<usize> {
        println!("Fpath {}, {:X} {}", id, buf.as_ptr() as usize, buf.len());
        let files = self.files.lock();
        if let Some(file) = files.get(&id) {
            let name = self.name.as_bytes();

            let mut i = 0;
            while i < buf.len() && i < name.len() {
                buf[i] = name[i];
                i += 1;
            }
            if i < buf.len() {
                buf[i] = b':';
                i += 1;
            }
            if i < buf.len() {
                buf[i] = b'/';
                i += 1;
            }

            file.path(&mut buf[i..]).map(|count| i + count)
        } else {
            Err(Error::new(EBADF))
        }
    }

    fn frename(&self, id: usize, url: &[u8], uid: u32, _gid: u32) -> Result<usize> {
        let path = str::from_utf8(url).unwrap_or("").trim_matches('/');

        // println!("Frename {}, {} from {}, {}", id, path, uid, gid);

        let mut files = self.files.lock();
        if let Some(file) = files.get_mut(&id) {
            //TODO: Check for EINVAL
            // The new pathname contained a path prefix of the old, or, more generally,
            // an attempt was made to make a directory a subdirectory of itself.

            let mut last_part = String::new();
            for part in path.split('/') {
                if ! part.is_empty() {
                    last_part = part.to_string();
                }
            }
            if last_part.is_empty() {
                return Err(Error::new(EPERM));
            }

            let mut fs = self.fs.borrow_mut();

            let mut orig = file.get_dirent()?;


            if ! self.owner(uid) {
                // println!("orig not owned by caller {}", uid);
                return Err(Error::new(EACCES));
            }
            from(Dir::rename(&mut orig, path, &mut fs).map(|_x| 0 as usize))?;
            file.set_dirent(orig.clone())
            /*
            let mut nodes = Vec::new();
            let node_opt = self.path_nodes(&mut fs, path, uid, gid, &mut nodes)?;

            if let Some(parent) = nodes.last() {
                /*if ! parent.1.owner(uid) {
                    // println!("parent not owned by caller {}", uid);
                    return Err(Error::new(EACCES));
                }*/

                if let Some(ref node) = node_opt {
                    if ! node.1.owner(uid) {
                        // println!("new dir not owned by caller {}", uid);
                        return Err(Error::new(EACCES));
                    }

                    if node.1.is_dir() {
                        if ! orig.1.is_dir() {
                            // println!("orig is file, new is dir");
                            return Err(Error::new(EACCES));
                        }

                        let mut children = Vec::new();
                        fs.child_nodes(&mut children, node.0)?;

                        if ! children.is_empty() {
                            // println!("new dir not empty");
                            return Err(Error::new(ENOTEMPTY));
                        }
                    } else {
                        if orig.1.is_dir() {
                            // println!("orig is dir, new is file");
                            return Err(Error::new(ENOTDIR));
                        }
                    }
                }

                let orig_parent = orig.1.parent;

                orig.1.set_name(&last_part)?;
                orig.1.parent = parent.0;

                if parent.0 != orig_parent {
                    fs.remove_blocks(orig.0, 1, orig_parent)?;
                }

                fs.write_at(orig.0, &orig.1)?;

                if let Some(node) = node_opt {
                    if node.0 != orig.0 {
                        fs.node_set_len(node.0, 0)?;
                        fs.remove_blocks(node.0, 1, parent.0)?;
                        fs.write_at(node.0, &Node::default())?;
                        fs.deallocate(node.0, BLOCK_SIZE)?;
                    }
                }

                if parent.0 != orig_parent {
                    fs.insert_blocks(orig.0, BLOCK_SIZE, parent.0)?;
                }

                Ok(0)
            } else {
                Err(Error::new(EPERM))
            }*/
        } else {
            Err(Error::new(EBADF))
        }
    }

    fn fstat(&self, id: usize, stat: &mut Stat) -> Result<usize> {
        println!("Fstat {}, {:X}", id, stat as *mut Stat as usize);
        let files = self.files.lock();
        if let Some(file) = files.get(&id) {
            file.stat(stat, &mut self.fs.borrow_mut())
        } else {
            Err(Error::new(EBADF))
        }
    }

    fn fstatvfs(&self, id: usize, stat: &mut StatVfs) -> Result<usize> {
        let files = self.files.lock();
        if let Some(_file) = files.get(&id) {
            let mut fs = self.fs.borrow_mut();

            /*let free = fs.header.1.free;
            let free_size = fs.node_len(free)?;*/
            let max_cluster = fs.max_cluster_number();
            stat.f_bsize = max_cluster.cluster_number as u32;
            stat.f_blocks = max_cluster.cluster_number - 1;
            stat.f_bfree = from(get_free_count(&mut fs, max_cluster).map(|x| x as usize))? as u64;
            stat.f_bavail = stat.f_bfree;

            Ok(0)
        } else {
            Err(Error::new(EBADF))
        }
    }

    fn fsync(&self, id: usize) -> Result<usize> {
        println!("Fsync {}", id);
        let mut files = self.files.lock();
        if let Some(file) = files.get_mut(&id) {
            file.sync(&mut self.fmaps.lock(), &mut self.fs.borrow_mut())
        } else {
            Err(Error::new(EBADF))
        }
    }

    fn ftruncate(&self, id: usize, len: usize) -> Result<usize> {
        println!("Ftruncate {}, {}", id, len);
        let mut files = self.files.lock();
        if let Some(file) = files.get_mut(&id) {
            file.truncate(len, &mut self.fs.borrow_mut())
        } else {
            Err(Error::new(EBADF))
        }
    }

    fn futimens(&self, id: usize, times: &[TimeSpec]) -> Result<usize> {
        println!("Futimens {}, {}", id, times.len());
        let mut files = self.files.lock();
        if let Some(file) = files.get_mut(&id) {
            file.utimens(times, self.mount_uid, &mut self.fs.borrow_mut())
        } else {
            Err(Error::new(EBADF))
        }
    }

    fn fmap(&self, id: usize, map: &Map) -> Result<usize> {
        println!("Fmap {}, {:?}", id, map);
        let mut files = self.files.lock();
        if let Some(file) = files.get_mut(&id) {
            file.fmap(map, &mut self.fmaps.lock(), &mut self.fs.borrow_mut())
        } else {
            Err(Error::new(EBADF))
        }
    }

    fn close(&self, id: usize) -> Result<usize> {
        println!("Close {}", id);
        let mut files = self.files.lock();
        if let Some(mut file) = files.remove(&id) {
            let _ = file.funmap(&mut self.fmaps.lock(), &mut self.fs.borrow_mut());
            Ok(0)
        } else {
            Err(Error::new(EBADF))
        }
    }

}