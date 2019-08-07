use std::io::{Read, Write, Seek, SeekFrom};
use std::iter::{Iterator, FromIterator};
use std::io::{ErrorKind, Error};
use std::{num, fmt, str};
use std::cmp::min;
use std::char;

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt, ByteOrder};

use Cluster;
use filesystem::FileSystem;
use table::{FatEntry, get_entry, allocate_cluster, deallocate_cluster_chain};

use super::Result;

pub const DIR_ENTRY_LEN: u64 = 32;
pub const LFN_PART_LEN: usize = 13;
// Max 32-bit unsigned value
pub const MAX_FILE_SIZE: u64 = 0xffffffff;

bitflags! {
    #[derive(Default)]
    pub struct FileAttributes: u8 {
        const RD_ONLY   = 0x01;
        const HIDDEN    = 0x02;
        const SYSTEM    = 0x04;
        const VOLUME_ID = 0x08;
        const DIRECTORY = 0x10;
        const ARCHIVE   = 0x20;
        const LFN       = Self::RD_ONLY.bits | Self::HIDDEN.bits
                            | Self::SYSTEM.bits | Self::VOLUME_ID.bits;
   }
}

#[derive(Debug, Default, Clone)]
pub struct File {
    pub first_cluster : Cluster,
    pub file_path : String,
    pub fname: String,
    pub short_dir_entry: ShortDirEntry,
    /// Starting and ending offsets of directory entries
    pub loc: ((Cluster, u64), (Cluster, u64))
    // FIXME: Add pointer to directory entry
}

#[derive(Debug, Default, Clone)]
pub struct Dir {
    pub first_cluster: Cluster,
    pub root_offset: Option<u64>,
    pub dir_path: String,
    pub dir_name: String,
    pub short_dir_entry: Option<ShortDirEntry>,
    pub loc: Option<((Cluster, u64), (Cluster, u64))>
}

impl Dir {
    pub fn to_iter<'a, D: Read + Write + Seek>(&self, fs: &'a mut FileSystem<D>) -> DirIter<'a, D> {
        DirIter {
            current_cluster: self.first_cluster,
            dir_path: self.dir_path.clone(),
            offset: self.root_offset.unwrap_or(0),
            is_root: self.is_root(),
            fs: fs
        }
    }

    // Is root dir of fat12 and fat16
    pub fn is_root(&self) -> bool {
        self.root_offset.is_some()
    }

    pub fn size<D: Read + Write + Seek>(&self, fs: &mut FileSystem<D>) -> u64 {
        fs.num_clusters_chain(self.first_cluster) * fs.bytes_per_cluster()
    }


    pub fn find_free_entries<D: Read + Write + Seek>(&self, num_free: u64, fs: &mut FileSystem<D>) -> Result<Option<(Cluster, u64)>> {
        let mut free = 0;
        let mut current_cluster = self.first_cluster;
        let mut offset = self.root_offset.unwrap_or(0);
        let mut first_free = None;

        loop {
            if offset >= fs.bytes_per_cluster() && !self.is_root() {
                match get_entry(fs, current_cluster).ok() {
                    Some(FatEntry::Next(c)) => {
                        current_cluster = c;
                        offset = offset % fs.bytes_per_cluster();
                    },
                    _ => {
                        break;
                    }
                }
            }

            if self.is_root() && offset > fs.root_dir_end_offset().unwrap() {
                return Ok(None)
            }

            let e_offset = fs.cluster_offset(current_cluster) + offset;
            let entry = get_dir_entry_raw(fs, e_offset)?;
            match entry {
                DirEntryRaw::Free | DirEntryRaw::FreeRest => {
                    if free == 0 {
                        first_free = Some((current_cluster, offset));
                    }
                    free += 1;
                    if free == num_free {
                        return Ok(first_free)
                    }
                },
                _ => {
                    free = 0;
                }
            }
            offset += DIR_ENTRY_LEN;
        }

        // FIXME

        let remaining = num_free - free;
        let clusters_req = (remaining * DIR_ENTRY_LEN + fs.bytes_per_cluster() - 1) / fs.bytes_per_cluster();
        let mut first_cluster = Cluster::default();
        let mut prev_cluster = current_cluster;
        for i in 0..clusters_req {
            let c = allocate_cluster(fs, Some(prev_cluster))?;
            if i == 0 {
                first_cluster = c;
            }
            prev_cluster = c;
        }

        if free > 0 {
            Ok(first_free)
        }
        else {
            Ok(Some((first_cluster, 0)))
        }

    }
     //pub fn find_entry(&self, name: &str, )
    // TODO: open, create_file, create_dir, find_entry
    pub fn find_entry<D: Read + Write + Seek>(&self, name: &str,
                      expected_dir: Option<bool>,
                      mut short_name_gen: Option<&mut ShortNameGen>, fs: &mut FileSystem<D>) -> Result<DirEntry> {
         for e in self.to_iter(fs) {
             if e.eq_name(name) {
                 if expected_dir.is_some() && Some(e.is_dir()) != expected_dir {
                     let msg = if e.is_dir() { "Is a directory" } else { "Is a file" };
                     return Err(Error::new(ErrorKind::Other, msg));
                 }
                 return Ok(e);
             }

             if let Some(ref mut sng) = short_name_gen {
                 sng.add_name(&e.short_name_raw())
             }
         }
         Err(Error::new(ErrorKind::NotFound, "No such file or directory"))
     }

    pub fn open_file<D: Read + Write + Seek>(&self, path: &str, fs: &mut FileSystem<D>) -> Result<File> {
        let (name, rest) = split_path(path);
        match rest {
            Some(r) => {
                let e = self.find_entry(name, Some(true), None, fs)?;
                e.to_dir().open_file(path, fs)
            },
            None => {
                let e = self.find_entry(name, Some(false), None, fs)?;
                Ok(e.to_file())
            }
        }
    }

    pub fn open_dir<D: Read + Write + Seek>(&self, path: &str, fs: &mut FileSystem<D>) -> Result<Dir> {
        let (name, rest) = split_path(path);
        let e = self.find_entry(name, Some(true), None, fs)?;
        match rest {
            Some(r) => {
                e.to_dir().open_dir(r, fs)
            },
            None => {
                Ok(e.to_dir())
            }
        }
    }

    pub fn create_file<D: Read + Write + Seek>(&self, path: &str, fs: &mut FileSystem<D>) -> Result<File> {
        let (name , rest) = split_path(path);
        if let Some(r) = rest {
            return self.find_entry(name, Some(true), None, fs)?.to_dir().create_file(r, fs);
        }

        let r = self.check_existence(name, Some(false), fs)?;
        match r {
            DirEntryOrShortName::ShortName(short_name) => {
                self.create_dir_entries(name, &short_name, None,
                                              FileAttributes::ARCHIVE, fs).map(|e| e.to_file())
            },
            DirEntryOrShortName::DirEntry(e) => Ok(e.to_file())
        }

    }

    pub fn create_dir<D: Read + Write + Seek>(&self, path: &str, fs: &mut FileSystem<D>) -> Result<Dir> {
        let (name , rest) = split_path(path);
        if let Some(r) = rest {
            return self.find_entry(name, Some(true), None, fs)?.to_dir().create_dir(r, fs);
        }

        let r = self.check_existence(name, Some(true), fs)?;
        match r {
            DirEntryOrShortName::ShortName(short_name) => {
                let mut short_entry = ShortDirEntry::default();
                let f_cluster = allocate_cluster(fs, None)?;
                short_entry.set_first_cluster(f_cluster);

                let mut offset = 0;
                let mut dot_entry = ShortDirEntry::default();
                dot_entry.dir_name = ShortNameGen::new(".").generate().unwrap();
                dot_entry.file_attrs = FileAttributes::DIRECTORY;
                dot_entry.set_first_cluster(f_cluster);
                dot_entry.flush(fs.cluster_offset(f_cluster) + offset, fs)?;
                //TODO Set time
                offset += DIR_ENTRY_LEN;

                let mut dot_entry = ShortDirEntry::default();
                dot_entry.dir_name = ShortNameGen::new("..").generate().unwrap();
                dot_entry.file_attrs = FileAttributes::DIRECTORY;
                dot_entry.set_first_cluster(self.first_cluster);
                //TODO Set Time
                dot_entry.flush(fs.cluster_offset(f_cluster) + offset, fs)?;


                self.create_dir_entries(name, &short_name, Some(short_entry),
                                        FileAttributes::DIRECTORY, fs).map(|e| e.to_dir())
            },
            DirEntryOrShortName::DirEntry(e) => Ok(e.to_dir())
        }
    }

    fn check_existence<D: Read + Write + Seek>(&self, name: &str, expected_dir: Option<bool>,
                                               fs: &mut FileSystem<D>) -> Result<DirEntryOrShortName> {
        let mut sng = ShortNameGen::new(name);
        loop {
            let e = self.find_entry(name, expected_dir, Some(&mut sng), fs);
            match e {
                Err(ref e) if e.kind() == ErrorKind::NotFound => {},
                Err(err) => return Err(err),
                Ok(e) => return Ok(DirEntryOrShortName::DirEntry(e))
             }
            if let Ok(name) = sng.generate() {
                return Ok(DirEntryOrShortName::ShortName(name))
            }
            sng.next_iteration();
        }

    }

    fn create_dir_entries<D: Read + Write + Seek>(&self, lname: &str, sname: &[u8; 11],
                                                  short_entry: Option<ShortDirEntry>,
                                                  fattrs: FileAttributes, fs: &mut FileSystem<D>) -> Result<DirEntry> {
        let mut short_entry = short_entry.unwrap_or(ShortDirEntry::default());
        short_entry.dir_name = sname.clone();
        short_entry.file_attrs = fattrs;
        //TODO: Modification/Creation Time

        let mut lng = LongNameEntryGenerator::new(lname, short_entry.compute_checksum());
        let num_entries = lng.num_entries() as u64 + 1;
        let free_entries = self.find_free_entries(num_entries, fs)?;
        let start_loc = match free_entries {
            Some(c) => c,
            None => return Err(Error::new(ErrorKind::Other, "No space left in dir/disk"))
        };

        let offsets: Vec<(Cluster, u64)> = DirEntryOffsetIter::new(start_loc, fs, num_entries, None).collect();
        for off in &offsets.as_slice()[..offsets.len() - 1] {
            let le: LongDirEntry = lng.next().unwrap(); // SAFE
            let offset = fs.cluster_offset(off.0) + off.1;
            le.flush(offset, fs)?;
        }

        let start = offsets[0];
        let end = *offsets.last().unwrap();
        let offset = fs.cluster_offset(end.0) + end.1;
        short_entry.flush(offset, fs)?;
        Ok(short_entry.to_dir_entry_lfn(lname.to_string(), (start, end), &self.dir_path))
    }

    fn is_empty<D: Read + Write + Seek>(&self, fs: &mut FileSystem<D>) -> bool {
        for e in self.to_iter(fs) {
            let s = e.short_name();
            if s == "." || s == ".." {
                continue;
            }
            return false
        }
        true
    }


    pub fn remove<D: Read + Write + Seek>(&self, path: &str, fs: &mut FileSystem<D>) -> Result<()> {
        let (name, rest) = split_path(path);
        if let Some(r) = rest {
            return self.find_entry(name, Some(true), None, fs)?.to_dir().remove(r, fs);
        }

        let e = self.find_entry(name, None, None, fs)?;
        if e.is_dir() && !e.to_dir().is_empty(fs) {
            return Err(Error::new(ErrorKind::Other, "Directory not empty"));
        }

        if e.first_cluster().cluster_number >= 2 {
            deallocate_cluster_chain(fs, e.first_cluster())?
        }

        if e.get_dir_range().is_some() {
            self.remove_dir_entries(e.get_dir_range().unwrap(), fs)?
        }

        Ok(())

    }

    fn remove_dir_entries<D: Read + Write + Seek>(&self, rng: ((Cluster, u64), (Cluster, u64)),
                                                  fs: &mut FileSystem<D>) -> Result<()> {
        let offsets: Vec<(Cluster, u64)> = DirEntryOffsetIter::new(rng.0, fs, 15, Some(rng.1)).collect();
        for off in offsets {
            let offset = fs.cluster_offset(off.0) + off.1;
            let mut s_entry = ShortDirEntry::default();
            s_entry.dir_name[0] = 0xe5;
            s_entry.flush(offset, fs)?;
        }
        Ok(())
    }

    pub fn get_entry<D: Read + Write + Seek>(&self, path: &str, fs: &mut FileSystem<D>) -> Result<DirEntry> {
        let (name, rest) = split_path(path);

        match rest {
            Some(r) => {
                let e = self.find_entry(name, Some(true), None, fs)?;
                e.to_dir().get_entry(r, fs)
            },
            None => {
                // If path was "/" then return the current dir
                if name.len() == 0 {
                    return Ok(DirEntry::Dir(self.clone()))
                }
                self.find_entry(name, None, None, fs)
            }
        }
    }

    pub fn get_entry_abs<D: Read + Write + Seek>(path: &str, fs: &mut FileSystem<D>) -> Result<DirEntry> {
        let root_dir = fs.root_dir();
        //println!("Getting abs entry for {:?}", path);
        root_dir.get_entry(path, fs)
    }

    //TODO: Check if src_path is an ancestor of dst_path
    pub fn rename<D: Read + Write + Seek>(src_entry: &DirEntry, dst_path: &str, fs: &mut FileSystem<D>) -> Result<()> {
        /*let (src_file, src_dir_path) = rsplit_path(src_path);
        let src_entry = self.get_entry(src_path, fs)?;
        let src_dir = match src_dir_path {
            None => self.clone(),
            Some(r) => {
                match self.get_entry(r, fs)? {
                    Ok(DirEntry::Dir(d)) => DirEntry::Dir(d),
                    Err(e) => return Err(e),
                    _ => return Err(Error::new(ErrorKind::Other, "Invalid source path"))
                }
            }
        };*/

        let (dst_name, dst_dir_path) = rsplit_path(dst_path);


        let dst_dir = match dst_dir_path {
            None => fs.root_dir(),
            Some(r) => {
                match Self::get_entry_abs(r, fs) {
                    Ok(DirEntry::Dir(d)) => d,
                    // Parent dir not found
                    Err(e) => return Err(e),
                    //Not a Directory
                    _ => return Err(Error::new(ErrorKind::Other, "Invalid destination path"))
                }
            }
        };

        let parent_dir_path = src_entry.dir_path();
        let src_dir = match Self::get_parent(parent_dir_path.as_str(), fs) {
            Ok(Some(d)) => d,
            _ => return Err(Error::new(ErrorKind::Other, "Src directory not found"))
        };

        // Ensures src and dst are of the same type
        match dst_dir.check_existence(dst_name, Some(src_entry.is_dir()), fs)? {
            DirEntryOrShortName::DirEntry(e) => {
                let s_name = e.short_name_raw();
                dst_dir.remove(dst_name, fs)?;
                match e {
                    DirEntry::File(f) | DirEntry::VolID(f) => {
                        let short_entry = src_entry.short_dir_entry().unwrap();
                        //TODO: Modification time
                        dst_dir.create_dir_entries(f.fname.as_str(), &s_name, Some(short_entry), short_entry.file_attrs, fs)?;
                        src_dir.remove(src_entry.name().as_str(), fs)?;

                    },
                    DirEntry::Dir(d) => {
                        let mut short_entry = src_entry.short_dir_entry();
                        if let Some(se) = short_entry {
                            dst_dir.create_dir_entries(d.dir_name.as_str(), &s_name, Some(se), se.file_attrs, fs)?;
                            src_dir.remove(src_entry.name().as_str(), fs)?;
                        }
                        else {
                            return Err(Error::new(ErrorKind::PermissionDenied, "Cannot move root dir"));
                        }


                    }

                }

            },
            DirEntryOrShortName::ShortName(s) => {
                println!("Creating a new Entry");
                let mut short_entry = src_entry.short_dir_entry();
                if let Some(se) = short_entry {
                    dst_dir.create_dir_entries(dst_name, &s, Some(se), se.file_attrs, fs)?;
                    src_dir.remove(src_entry.name().as_str(), fs)?;
                }
                else {
                    return Err(Error::new(ErrorKind::PermissionDenied, "Cannot move root dir"));
                }

            }
        };

        Ok(())




        /*
        if src_entry.is_file() && dst_entry.is_dir() {
            return Err(Error::new(ErrorKind::Other, "Cannot move file to directory"))
        }

        if src_entry.is_dir() && dst_entry.is_file() {
            return Err(Error::new(ErrorKind::Other, "Cannot move directory to file"))
        }*/

        /*
        let existing = dst_dir.check_existence(dst_file, None, fs)?;
        let short_name = match r {
            DirEntryOrShortName::DirEntry(ref dst_e) => {
                // check if source and destination entry is the same

                return Err(Error::new(ErrorKind::AlreadyExists, "Destination file already exists"));
            },
            DirEntryOrShortName::ShortName(short_name) => short_name,
        };*/

    }

    fn get_parent<D: Read + Write + Seek>(abs_path: &str, fs: &mut FileSystem<D>) -> Result<Option<Dir>> {
        let root_dir = fs.root_dir();
        let (_, parent_path) = rsplit_path(abs_path);
        println!("Parent dir path: {:?} for abs path : {:?}", parent_path, abs_path);
        match parent_path {
            Some(p) => root_dir.get_entry(p, fs).map(|x|
                if x.is_dir() { Some(x.to_dir()) } else { None }),
            None => Ok(Some(root_dir))
        }

    }
}

struct DirEntryOffsetIter<'a, D: Read + Write + Seek> {
    start_offset: (Cluster, u64),
    current_offset: (Cluster, u64),
    end_offset: Option<(Cluster, u64)>,
    fs: &'a mut FileSystem<D>,
    idx: u64,
    len: u64,
    fin: bool
}

impl<'a, D: Read + Write + Seek> DirEntryOffsetIter<'a, D> {
    fn new(start: (Cluster, u64), fs: &'a mut FileSystem<D>,
           len: u64, end_offset: Option<(Cluster, u64)>) -> DirEntryOffsetIter<'a, D> {
        DirEntryOffsetIter {
            start_offset: start,
            current_offset: start,
            end_offset,
            fs,
            idx: 0,
            len,
            fin: false
        }
    }
}

impl<'a, D: Read + Write + Seek> Iterator for DirEntryOffsetIter<'a, D> {
    type Item = (Cluster, u64);
    fn next(&mut self) -> Option<Self::Item> {
        if self.idx == self.len || self.fin {
            return None
        }

        let r = self.current_offset;
        let mut new_offset = r.1 + DIR_ENTRY_LEN;
        let mut new_cluster = r.0;
        if new_offset >= self.fs.bytes_per_cluster() {
            new_offset = new_offset % self.fs.bytes_per_cluster();
            match get_entry(self.fs, new_cluster) {
                Ok(FatEntry::Next(c)) => {
                    new_cluster = c;

                },
                _ => {
                    return None;
                }
            }
        }
        if let Some(off) = self.end_offset {
            self.fin = off == self.current_offset;
        }

        self.current_offset = (new_cluster, new_offset);
        self.idx += 1;



        Some(r)
    }
}



impl File {
    pub fn size(&self) -> u64 {
        self.short_dir_entry.file_size as u64
    }

    pub fn set_size(&mut self, sz: u32) {
        self.short_dir_entry.file_size = sz;
    }

    pub fn read<D: Read + Write + Seek>(&self, buf: &mut [u8], fs: &mut FileSystem<D>, mut offset: u64) -> Result<usize> {
        if offset >= self.size() {
            return Ok(0)
        }

        let start_cluster_number = offset / fs.bytes_per_cluster();
        let mut current_cluster = match fs.get_cluster_relative(self.first_cluster, start_cluster_number as usize) {
            Some(c) => c,
            None => return Ok(0)
        };

        let bytes_remaining_file = self.size() - offset;
        let read_size = min(buf.len(), bytes_remaining_file as usize);
        let mut cluster_offset = offset % fs.bytes_per_cluster();

        let mut start = 0;
        let mut read = 0;

        loop {
            if cluster_offset >= fs.bytes_per_cluster() {
                match get_entry(fs, current_cluster).ok() {
                    Some(FatEntry::Next(c)) => {
                        current_cluster = c;
                        cluster_offset = cluster_offset % fs.bytes_per_cluster();
                    },
                    _ => {
                        break;
                    }
                }
            }
            let end_len = min(min((fs.bytes_per_cluster() - cluster_offset) as usize, buf.len() - read), read_size - read);
            let r = fs.read_at(fs.cluster_offset(current_cluster) + cluster_offset, &mut buf[start..start + end_len])?;
            read += r;
            start += r;
            cluster_offset += r as u64;
            if read == read_size {
                break;
            }

        }
        Ok(read)

    }

    pub fn write<D: Read + Write + Seek>(&mut self, buf: &[u8], fs: &mut FileSystem<D>, mut offset: u64) -> Result<usize> {
        self.ensure_len(offset, buf.len() as u64, fs)?;

        //FIXME
        let start_cluster_number = offset / fs.bytes_per_cluster();
        let mut current_cluster = match fs.get_cluster_relative(self.first_cluster, start_cluster_number as usize) {
            Some(c) => c,
            None => return Ok(0)
        };
        //println!("Over here!");

        let mut cluster_offset = offset % fs.bytes_per_cluster();

        let mut start = 0;
        let mut written = 0;

        loop {
            if cluster_offset >= fs.bytes_per_cluster() {
                match get_entry(fs, current_cluster).ok() {
                    Some(FatEntry::Next(c)) => {
                        current_cluster = c;
                        cluster_offset = cluster_offset % fs.bytes_per_cluster();
                    },
                    _ => {
                        break;
                    }
                }
            }

            let end_len = min((fs.bytes_per_cluster() - cluster_offset) as usize, buf.len() - written);
            println!("Cluster = {:?}, Cluster Offset = {:?}, Cluster Size = {:?}, start = {:?}, end = {:?}",
                     current_cluster, cluster_offset, fs.bytes_per_cluster(), start, start + end_len);
            let w = fs.write_to(fs.cluster_offset(current_cluster) + cluster_offset, &buf[start..start + end_len])?;

            written += w;
            start += w;
            cluster_offset += w as u64;
            if written == buf.len() {
                break;
            }
        }

        Ok(written)

    }

    fn ensure_len<D: Read + Write + Seek>(&mut self, offset: u64, len: u64, fs: &mut FileSystem<D>) -> Result<()> {
        if offset + len <= self.size() {
            return Ok(())
        }

        if self.size() == 0 {
            self.first_cluster = allocate_cluster(fs, None)?;
            self.short_dir_entry.set_first_cluster(self.first_cluster);
        }

        //Compute space available in last cluster
        let cluster_offset = self.size() % fs.bytes_per_cluster();
        let bytes_remaining_cluster = fs.bytes_per_cluster() - cluster_offset;

        //Compute bytes to be allocated
        let extra_bytes = min((offset + len) - self.size(), MAX_FILE_SIZE - self.size());

        // Allocate extra clusters as required
        if bytes_remaining_cluster < extra_bytes {
            let clusters_req = (extra_bytes - bytes_remaining_cluster + fs.bytes_per_cluster() - 1) / fs.bytes_per_cluster();
            let last_cluster = match fs.get_last_cluster(self.first_cluster) {
                Some(c) => c,
                None => return Err(Error::new(ErrorKind::InvalidData, "Last Cluster not found"))
            };

            let mut current_cluster = last_cluster;
            for i in 0..clusters_req {
                println!("[info] Allocating Cluster for length req");
                current_cluster = allocate_cluster(fs, Some(current_cluster))?;
            }
        }

        //TODO: Optimize
        if offset > self.size() {
            let cluster_start = self.size() / fs.bytes_per_cluster();
            let s_cluster = fs.get_cluster_relative(self.first_cluster, cluster_start as usize).unwrap();
            let start_offset = fs.cluster_offset(s_cluster) + self.size() % fs.bytes_per_cluster();
            let b_remaining = fs.bytes_per_cluster() - (self.size() % fs.bytes_per_cluster());
            let offset_start = offset / fs.bytes_per_cluster();
            let offset_cluster = fs.get_cluster_relative(self.first_cluster, offset_start as usize).unwrap();
            if s_cluster != offset_cluster {
                self.zero_range(fs, start_offset, start_offset + b_remaining - 1)?;
            } else {
                self.zero_range(fs, start_offset, start_offset + offset - self.size() - 1)?;
            }
        }


        let new_size = self.size() + extra_bytes;
        // TODO: Add mod time and other stuff
        self.set_size(new_size as u32);
        let short_entry_offset = fs.cluster_offset((self.loc.1).0) + (self.loc.1).1;
        self.short_dir_entry.flush(short_entry_offset, fs)?;

        Ok(())

    }

    // Range start and range end are absolute disk offsets
    // Range start and range end must be in the same cluster
    fn zero_range<D: Read + Write + Seek>(&self, fs: &mut FileSystem<D>, range_start: u64, range_end: u64) -> Result<()> {
        if range_end < range_start {
            return Ok(())
        }

        println!("Zeroing Range: {} - {}", range_start, range_end);
        let zeroes = vec![0; (range_end - range_start + 1) as usize];
        fs.write_to(range_start, zeroes.as_slice())?;
        Ok(())
    }



    pub fn truncate<D: Read + Write + Seek>(&mut self, fs: &mut FileSystem<D>, new_size: u64) -> Result<()> {
        if new_size >= self.size() {
            return Ok(())
        }

        let new_last_cluster = new_size / fs.bytes_per_cluster();
        match fs.get_cluster_relative(self.first_cluster, (new_last_cluster + 1) as usize) {
            Some(c) => {
                deallocate_cluster_chain(fs, c)?;
            },
            None => { }
        }

        self.set_size(new_size as u32);
        let short_entry_offset = fs.cluster_offset((self.loc.1).0) + (self.loc.1).1;
        self.short_dir_entry.flush(short_entry_offset, fs)?;
        Ok(())

    }
}

#[derive(Debug, Default, Copy, Clone)]
pub struct ShortDirEntry {
    /// Short name
    dir_name: [u8; 11],
    /// File Attributes
    file_attrs: FileAttributes,
    /// Win NT reserved
    nt_res: u8,
    /// Millisecond part of file creation time
    crt_time_tenth: u8,
    /// Time of file creation
    crt_time: u16,
    /// Date of file creation
    crt_date: u16,
    /// Last access date
    lst_acc_date: u16,
    /// High word of first cluster(0 for FAT12 and FAT16)
    fst_clst_hi: u16,
    /// Last write time
    wrt_time: u16,
    /// Last write date
    wrt_date: u16,
    /// Low word of first cluster
    fst_clus_lo: u16,
    /// File Size
    file_size: u32
}

#[derive(Debug, Default, Copy, Clone)]
pub struct LongDirEntry {
    /// Ordinal of the entry
    ord: u8,
    /// Characters 1-5 of name
    name1: [u16; 5],
    /// File Attributes
    file_attrs: FileAttributes,
    /// Entry Type: If zero indicates that the entry
    /// is a subcomponent of a long name
    /// Non-zero values are reserved
    dirent_type: u8,
    /// Checksum computed from short name
    chksum: u8,
    /// Characters 6-11 of name
    name2: [u16; 6],
    /// FirstCluster Low Word
    /// Should be zero in a long file entry
    first_clus_low: u16,
    /// Characters 12-13 of name
    name3: [u16; 2]
}

impl LongDirEntry {
    pub fn is_last(&self) -> bool {
        self.ord & 0x40 > 0
    }

    pub fn copy_name_to_slice(&self, name_part: &mut [u16]) {
        assert_eq!(name_part.len(), LFN_PART_LEN);
        name_part[0..5].copy_from_slice(&self.name1);
        name_part[5..11].copy_from_slice(&self.name2);
        name_part[11..13].copy_from_slice(&self.name3);
    }

    fn insert_name(&mut self, name_part: &[u16]) {
        assert_eq!(name_part.len(), LFN_PART_LEN);
        self.name1.copy_from_slice(&name_part[0..5]);
        self.name2.copy_from_slice(&name_part[5..11]);
        self.name3.copy_from_slice(&name_part[11..13]);
    }

    pub fn order(&self) -> u8 {
        self.ord
    }

    pub fn chksum(&self) -> u8 {
        self.chksum
    }

    fn new(ord: u8, name_part: &[u16], checksum: u8) -> Self {
        let mut lentry = LongDirEntry::default();
        lentry.ord = ord;
        lentry.insert_name(name_part);
        lentry.file_attrs = FileAttributes::LFN;
        lentry.dirent_type = 0;
        lentry.chksum = checksum;
        lentry.first_clus_low = 0;
        lentry
    }

    fn flush<D: Read + Write + Seek>(&self, offset: u64, fs: &mut FileSystem<D>) -> Result<()> {
        fs.seek_to(offset)?;
        fs.disk.borrow_mut().write_u8(self.ord)?;
        //fs.disk.borrow_mut().write_u16_into::<LittleEndian>(&self.name1)?;
        for b in &self.name1 {
            fs.disk.borrow_mut().write_u16::<LittleEndian>(*b);
        }

        fs.disk.borrow_mut().write_u8(self.file_attrs.bits);
        fs.disk.borrow_mut().write_u8(self.dirent_type);
        fs.disk.borrow_mut().write_u8(self.chksum);
        //fs.disk.borrow_mut().write_u16_into::<LittleEndian>(&self.name2)?;
        for b in &self.name2 {
            fs.disk.borrow_mut().write_u16::<LittleEndian>(*b);
        }
        fs.disk.borrow_mut().write_u16::<LittleEndian>(self.first_clus_low);
        //fs.disk.borrow_mut().write_u16_into::<LittleEndian>(&self.name3)?;
        for b in &self.name3 {
            fs.disk.borrow_mut().write_u16::<LittleEndian>(*b);
        }
        Ok(())
    }
}

impl ShortDirEntry {
    const PADDING: u8 = ' ' as u8;

    pub fn is_dir(&self) -> bool {
        self.file_attrs.contains(FileAttributes::DIRECTORY) &&
            !self.file_attrs.contains(FileAttributes::VOLUME_ID)
    }

    pub fn is_file(&self) -> bool {
        !self.file_attrs.contains(FileAttributes::DIRECTORY) &&
            !self.file_attrs.contains(FileAttributes::VOLUME_ID)
    }

    pub fn is_vol_id(&self) -> bool {
        !self.file_attrs.contains(FileAttributes::DIRECTORY) &&
            self.file_attrs.contains(FileAttributes::VOLUME_ID)
    }


    /// Taken from rust-fatfs: https://github.com/rafalh/rust-fatfs
    fn name_to_string(&self) -> String {
        let sname_len = self.dir_name[..8].iter().rposition(|x| *x != Self::PADDING)
            .map(|l| l + 1).unwrap_or(0);
        let ext_len = self.dir_name[8..].iter().rposition(|x| *x != Self::PADDING)
            .map(|l| l + 1).unwrap_or(0);

        let mut name = [Self::PADDING; 12];
        name[..sname_len].copy_from_slice(&self.dir_name[..sname_len]);

        let tot_len = if ext_len > 0 {
            name[sname_len] = '.' as u8;
            name[sname_len + 1..sname_len + 1 + ext_len].copy_from_slice(&self.dir_name[8..8 + ext_len]);
            sname_len + 1 + ext_len
        } else {
            sname_len
        };

        if name[0] == 0x05 {
            name[0] = 0xe5;
        }
        let iter = name[..tot_len].iter().cloned().map(|c| char_decode(c));
        String::from_iter(iter)
    }

    pub fn to_dir_entry(&self, loc: (Cluster, u64), dir_path: &String) -> DirEntry {
        if self.is_file() || self.is_vol_id() {
            let mut file = File::default();
            let f_name = self.name_to_string();
            let mut f_path = dir_path.clone();

            f_path.push_str(&f_name.clone());
            f_path.push('/');
            let cluster = Cluster::new((self.fst_clus_lo as u64) | ((self.fst_clst_hi as u64) << 16));
            file.first_cluster = cluster;
            file.file_path = f_path;
            file.fname = f_name;
            file.short_dir_entry = self.clone();
            file.loc = (loc, loc);
            if self.is_file() {
                DirEntry::File(file)
            }
            else {
                DirEntry::VolID(file)
            }
        } else {
            let mut dir = Dir::default();
            let cluster = Cluster::new((self.fst_clus_lo as u64) | ((self.fst_clst_hi as u64) << 16));
            dir.first_cluster = cluster;
            let dir_name = self.name_to_string();
            let mut d_path = dir_path.clone();

            d_path.push_str(&dir_name.clone());
            d_path.push('/');
            dir.dir_path = d_path;
            dir.dir_name = dir_name;
            dir.root_offset = None;
            dir.short_dir_entry = Some(self.clone());
            dir.loc = Some((loc, loc));
            DirEntry::Dir(dir)
        }

    }

    pub fn to_dir_entry_lfn(&self, name: String, loc: ((Cluster, u64), (Cluster, u64)), dir_path: &String) -> DirEntry {
        if self.is_file() || self.is_vol_id() {
            let mut file = File::default();
            let mut f_path = dir_path.clone();

            f_path.push_str(&name.clone());
            f_path.push('/');
            let cluster = Cluster::new((self.fst_clus_lo as u64) | ((self.fst_clst_hi as u64) << 16));
            file.first_cluster = cluster;
            file.file_path = f_path;
            file.fname = name;
            file.short_dir_entry = self.clone();
            file.loc = loc;
            if self.is_file() {
                DirEntry::File(file)
            }
            else {
                DirEntry::VolID(file)
            }
        } else {
            let mut dir = Dir::default();
            let cluster = Cluster::new((self.fst_clus_lo as u64) | ((self.fst_clst_hi as u64) << 16));
            dir.first_cluster = cluster;
            let mut d_path = dir_path.clone();

            d_path.push_str(&name.clone());
            d_path.push('/');
            dir.dir_path = d_path;
            dir.dir_name = name;
            dir.root_offset = None;
            dir.short_dir_entry = Some(self.clone());
            dir.loc = Some(loc);
            DirEntry::Dir(dir)
        }

    }

    fn compute_checksum(&self) -> u8 {
        let mut sum = num::Wrapping(0u8);
        for b in &self.dir_name {
            sum = (sum << 7) + (sum >> 1) + num::Wrapping(*b);
        }
        sum.0
    }

    pub fn flush<D: Read + Write + Seek>(&self, offset: u64, fs: &mut FileSystem<D>) -> Result<()> {
        fs.seek_to(offset)?;
        fs.disk.borrow_mut().write(&self.dir_name)?;
        fs.disk.borrow_mut().write_u8(self.file_attrs.bits)?;
        fs.disk.borrow_mut().write_u8(self.nt_res)?;
        fs.disk.borrow_mut().write_u8(self.crt_time_tenth)?;
        fs.disk.borrow_mut().write_u16::<LittleEndian>(self.crt_time)?;
        fs.disk.borrow_mut().write_u16::<LittleEndian>(self.crt_date)?;
        fs.disk.borrow_mut().write_u16::<LittleEndian>(self.lst_acc_date)?;
        fs.disk.borrow_mut().write_u16::<LittleEndian>(self.fst_clst_hi)?;
        fs.disk.borrow_mut().write_u16::<LittleEndian>(self.wrt_time)?;
        fs.disk.borrow_mut().write_u16::<LittleEndian>(self.wrt_date)?;
        fs.disk.borrow_mut().write_u16::<LittleEndian>(self.fst_clus_lo)?;
        fs.disk.borrow_mut().write_u32::<LittleEndian>(self.file_size)?;
        fs.disk.borrow_mut().flush()?;
        Ok(())
    }

    pub fn set_first_cluster(&mut self, cluster: Cluster) {
        self.fst_clus_lo = (cluster.cluster_number & 0x0000ffff) as u16;
        self.fst_clst_hi = ((cluster.cluster_number & 0xffff0000) >> 16) as u16;
    }

}

fn char_decode(c: u8) -> char {
    if c <= 0x7f {
        c as char
    } else {
        '\u{FFFD}'
    }
}

#[derive(Debug, Clone)]
pub enum DirEntryRaw {
    Short(ShortDirEntry),
    Long(LongDirEntry),
    Free,
    FreeRest
}

impl DirEntryRaw {
    fn is_last(&self) -> bool {
        match self {
            &DirEntryRaw::Short(s) => true,
            &DirEntryRaw::Long(l) => l.is_last(),
            _ => false
        }
    }

    fn is_long(&self) -> bool {
        match self {
            &DirEntryRaw::Long(_) => true,
            _ => false
        }
    }

    fn is_short(&self) -> bool {
        match self {
            &DirEntryRaw::Short(_) => true,
            _ => false
        }
    }

}

pub struct DirIter<'a, D: Read + Write + Seek> {
    current_cluster: Cluster,
    dir_path: String,
    offset: u64,
    /// True for the root directories of FAT12 and FAT16
    is_root: bool,
    fs: &'a mut FileSystem<D>,
}

impl<'a, D: Read + Write + Seek> Iterator for DirIter<'a, D> {
    type Item = DirEntry;
    fn next(&mut self) -> Option<Self::Item> {
        match self.get_dir_entry() {
            Ok((offset, cluster, ret)) => {
                self.offset = offset;
                self.current_cluster = cluster;
                ret
            },
            Err(e) => None
        }
    }
}

impl<'a, D: Read + Write + Seek> DirIter <'a, D>{
    fn get_dir_entry(&mut self) -> Result<(u64, Cluster, Option<DirEntry>)> {

        loop {
            if self.offset >= self.fs.bytes_per_cluster() && !self.is_root() {
                match get_entry(self.fs, self.current_cluster).ok() {
                    Some(FatEntry::Next(c)) => {
                        self.current_cluster = c;
                        self.offset = self.offset % self.fs.bytes_per_cluster();
                    },
                    _ => return Ok((self.offset, self.current_cluster, None))
                }
            }

            if self.is_root() && self.offset > self.fs.root_dir_end_offset().unwrap() {
                return Ok((self.offset, self.current_cluster, None))
            }

            let mut dentry = get_dir_entry_raw(self.fs, self.fs.cluster_offset(self.current_cluster) + self.offset)?;
            match dentry {
                DirEntryRaw::Short(s) => {
                    self.offset = self.offset + DIR_ENTRY_LEN;
                    return Ok((self.offset, self.current_cluster, Some(s.to_dir_entry((self.current_cluster, self.offset - DIR_ENTRY_LEN), &self.dir_path))))
                },
                DirEntryRaw::Long(l) => {
                    // Iterate till a short entry or a free entry
                    // Iterate only till 20 entries as the max file name size is 255
                    let mut lfn_entries = vec![dentry];
                    let start_offset = self.offset;
                    let start_cluster = self.current_cluster;

                    self.offset += DIR_ENTRY_LEN;

                    for i in 1..20 {
                        if self.offset >= self.fs.bytes_per_cluster() && !self.is_root() {
                            match get_entry(self.fs, self.current_cluster).ok() {
                                Some(FatEntry::Next(c)) => {
                                    self.current_cluster = c;
                                    self.offset = self.offset % self.fs.bytes_per_cluster();
                                },
                                _ => break
                            }
                        }

                        if self.is_root() && self.offset > self.fs.root_dir_end_offset().unwrap() {
                            break;
                        }

                        let mut dentry = get_dir_entry_raw(self.fs, self.fs.cluster_offset(self.current_cluster) + self.offset)?;
                        match dentry {
                            DirEntryRaw::Short(_) => {
                                lfn_entries.push(dentry);
                                break;
                            },
                            DirEntryRaw::Long(_) => {
                                lfn_entries.push(dentry);
                                self.offset += DIR_ENTRY_LEN;
                            },
                            _ => {
                                break;
                            }
                        }
                    }

                    let dir_entry = construct_dentry(lfn_entries, &self.dir_path, ((start_cluster, start_offset), (self.current_cluster, self.offset)));
                    match dir_entry {
                        Ok(d) => {
                            self.offset = self.offset + DIR_ENTRY_LEN;
                            return Ok((self.offset, self.current_cluster, Some(d)))
                        },
                        Err(_) => {
                            self.offset = self.offset + DIR_ENTRY_LEN;
                            //return self.get_dir_entry()
                        }
                    }
                },
                DirEntryRaw::Free => {
                    self.offset = self.offset + DIR_ENTRY_LEN;
                    //return self.get_dir_entry()
                },
                DirEntryRaw::FreeRest => {
                    return Ok((self.offset, self.current_cluster, None))
                }
            }
        }
    }

    fn is_root(&self) -> bool {
        self.is_root
    }
}

fn construct_dentry(mut lfn_entries: Vec<DirEntryRaw>, dir_path: &String, loc: ((Cluster, u64), (Cluster, u64))) -> Result<DirEntry> {
    if lfn_entries.len() == 0 {
        return Err(Error::new(ErrorKind::Other, "Empty lfn entries"))
    }

    if !lfn_entries[0].is_last() || !lfn_entries.last().unwrap().is_short() {
        return Err(Error::new(ErrorKind::Other, "Orphaned Entries"))
    }

    let short_entry = match lfn_entries.pop().unwrap() {
        DirEntryRaw::Short(s) => s,
        _ => unreachable!()
    };

    let mut name_builder = LongNameGen::new();
    for entry in &lfn_entries {
        match entry {
            &DirEntryRaw::Short(s) => {
                return Err(Error::new(ErrorKind::Other, "Orphaned Entries"))
            },
            &DirEntryRaw::Long(l) => {
                name_builder.process(l)?;
            },
            _ => return Err(Error::new(ErrorKind::Other, "Orphaned Entries"))
        }
    }

    name_builder.validate_checksum(&short_entry)?;
    let fname = name_builder.to_string();
    Ok(short_entry.to_dir_entry_lfn(fname, loc, dir_path))


}

pub fn get_dir_entry_raw<D: Read + Write + Seek>(fs: &mut FileSystem<D>, offset: u64) -> Result<DirEntryRaw> {
    fs.seek_to(offset)?;
    let dir_0 = fs.disk.borrow_mut().read_u8()?;
    match dir_0 {
        0x00 => Ok(DirEntryRaw::FreeRest),
        0xe5 => Ok(DirEntryRaw::Free),
        _ => {
            fs.disk.borrow_mut().seek(SeekFrom::Current(10))?;
            let f_attr: FileAttributes = FileAttributes::from_bits(fs.disk.borrow_mut().read_u8()?)
                .ok_or(Error::new(ErrorKind::Other, "Error Reading File Attr"))?;
            fs.seek_to(offset)?;
            if f_attr.contains(FileAttributes::LFN) {
                let mut ldr = LongDirEntry::default();
                ldr.ord = fs.disk.borrow_mut().read_u8()?;
                fs.disk.borrow_mut().read_u16_into::<LittleEndian>(&mut ldr.name1)?;
                ldr.file_attrs = FileAttributes::from_bits(fs.disk.borrow_mut().read_u8()?)
                    .ok_or(Error::new(ErrorKind::Other, "Error Reading File Attr"))?;
                ldr.dirent_type = fs.disk.borrow_mut().read_u8()?;
                ldr.chksum = fs.disk.borrow_mut().read_u8()?;
                fs.disk.borrow_mut().read_u16_into::<LittleEndian>(&mut ldr.name2)?;
                ldr.first_clus_low = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
                fs.disk.borrow_mut().read_u16_into::<LittleEndian>(&mut ldr.name3)?;
                Ok(DirEntryRaw::Long(ldr))
            } else {
                let mut sdr = ShortDirEntry::default();
                fs.disk.borrow_mut().read(&mut sdr.dir_name)?;
                sdr.file_attrs = FileAttributes::from_bits(fs.disk.borrow_mut().read_u8()?)
                    .ok_or(Error::new(ErrorKind::Other, "Error Reading File Attr"))?;
                sdr.nt_res = fs.disk.borrow_mut().read_u8()?;
                sdr.crt_time_tenth = fs.disk.borrow_mut().read_u8()?;
                sdr.crt_time = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
                sdr.crt_date = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
                sdr.lst_acc_date = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
                sdr.fst_clst_hi = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
                sdr.wrt_time = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
                sdr.wrt_date = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
                sdr.fst_clus_lo = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
                sdr.file_size = fs.disk.borrow_mut().read_u32::<LittleEndian>()?;
                Ok(DirEntryRaw::Short(sdr))
            }

        }
    }


}

#[derive(Debug, Clone)]
pub enum DirEntry {
    File(File),
    Dir(Dir),
    VolID(File)
}

pub enum DirEntryOrShortName {
    DirEntry(DirEntry),
    ShortName([u8; 11])
}

impl DirEntry {
    pub fn short_name(&self) -> String {
        match &self {
            &DirEntry::File(f) => {
                f.short_dir_entry.name_to_string()
            },
            &DirEntry::Dir(d) => {
                match d.short_dir_entry {
                    Some(s) => s.name_to_string(),
                    None => String::from("/")
                }
            },
            &DirEntry::VolID(s) => {
                s.short_dir_entry.name_to_string()
            }
        }
    }

    fn short_dir_entry(&self) -> Option<ShortDirEntry> {
        match &self {
            &DirEntry::File(f) => {
                Some(f.short_dir_entry)
            },
            &DirEntry::Dir(d) => {
                d.short_dir_entry
            },
            &DirEntry::VolID(s) => {
                Some(s.short_dir_entry)
            }
        }
    }

    fn first_cluster(&self) -> Cluster {
        match &self {
            &DirEntry::File(f) => {
                f.first_cluster
            },
            &DirEntry::Dir(d) => {
                d.first_cluster
            },
            &DirEntry::VolID(s) => {
                s.first_cluster
            }
        }
    }

    fn get_dir_range(&self) -> Option<((Cluster, u64), (Cluster, u64))>{
        match &self {
            &DirEntry::File(f) => {
                Some(f.loc)
            },
            &DirEntry::Dir(d) => {
                d.loc
            },
            &DirEntry::VolID(s) => {
                Some(s.loc)
            }
        }
    }

    fn short_name_raw(&self) -> [u8; 11] {
        match &self {
            &DirEntry::File(f) => f.short_dir_entry.dir_name,
            &DirEntry::Dir(d) => {
                match d.short_dir_entry {
                    Some(s) => s.dir_name,
                    None => {
                        let mut s = [0x20u8; 11];
                        s[0] = '/' as u8;
                        s
                    }
                }
            },
            &DirEntry::VolID(s) => s.short_dir_entry.dir_name
        }
    }

    fn name(&self) -> String {
        match &self {
            &DirEntry::File(f) => f.fname.clone(),
            &DirEntry::Dir(d) =>  d.dir_name.clone(),
            &DirEntry::VolID(s) => s.fname.clone()
        }
    }

    fn is_file(&self) -> bool {
        match self {
            &DirEntry::File(_) | &DirEntry::VolID(_) => true,
            _ => false
        }
    }

    fn is_dir(&self) -> bool {
        match &self {
            &DirEntry::Dir(d) => true,
            _ => false
        }
    }

    fn is_volID(&self) -> bool {
        match self {
            &DirEntry::VolID(_) => true,
            _ => false
        }
    }

    fn to_file(&self) -> File {
        assert!(self.is_file(), "Not a file");
        match &self {
            DirEntry::File(f) | DirEntry::VolID(f) => f.clone(),
            _ => unreachable!()
        }
    }

    fn to_dir(&self) -> Dir {
        assert!(self.is_dir(), "Not a directory");
        match &self {
            DirEntry::Dir(d) => d.clone(),
            _ => unreachable!()
        }
    }

    fn eq_name(&self, name: &str) -> bool {
        let short_name = self.short_name();
        let long_name = self.name();
        let long_name_upper = long_name.chars().flat_map(|c| c.to_uppercase());
        let name_upper = name.chars().flat_map(|c| c.to_uppercase());
        let long_name_matches = long_name_upper.eq(name_upper.clone());
        let short_name_upper = short_name.chars().flat_map(|c| c.to_uppercase());
        let short_name_matches = short_name_upper.eq(name_upper);
        long_name_matches || short_name_matches
    }

    fn dir_path(&self) -> String {
        match &self {
            &DirEntry::File(f) => f.file_path.clone(),
            &DirEntry::Dir(d) => d.dir_path.clone(),
            &DirEntry::VolID(s) => s.file_path.clone()
        }
    }


}

struct LongNameGen {
    name: Vec<u16>,
    chksum: u8,
    index: u8
}

impl LongNameGen {
    fn new() -> Self {
        LongNameGen {
            name: Vec::new(),
            chksum: 0,
            index: 0
        }
    }

    fn process(&mut self, lfn: LongDirEntry) -> Result<()> {
        let is_last = lfn.is_last();
        let index = lfn.order() & 0x1f;
        if index == 0 {
            self.name.clear();
            return Err(Error::new(ErrorKind::Other, "Orphaned Entries"))
        }
        if is_last {
            self.index = index;
            self.chksum = lfn.chksum();
            self.name.resize(index as usize * LFN_PART_LEN, 0);
        }
        else if self.index == 0 || index != self.index - 1 || self.chksum != lfn.chksum() {
            self.name.clear();
            return Err(Error::new(ErrorKind::Other, "Orphaned Entries"))
        } else {
            self.index -= 1;
        }
        let pos = (index - 1) as usize * LFN_PART_LEN;
        lfn.copy_name_to_slice(&mut self.name[pos..pos + LFN_PART_LEN]);
        Ok(())
    }

    fn len(&self) -> usize {
        self.name.len()
    }

    fn to_string(&self) -> String {
        let mut s = String::from_utf16_lossy(self.name.as_slice());
        let len = s.find('\u{0}').unwrap_or(s.len());
        s.truncate(len);
        s
    }

    fn validate_checksum(&self, short_entry: &ShortDirEntry) -> Result<()> {
        if self.chksum != short_entry.compute_checksum() {
            Err(Error::new(ErrorKind::Other, "Invalid Checksum"))
        } else {
            Ok(())
        }
    }
}

/// Taken from rust-fatfs: https://github.com/rafalh/rust-fatfs
fn split_path(path: &str) -> (&str, Option<&str>) {
    let mut path_split = path.trim_matches('/').splitn(2, "/");
    let comp = path_split.next().unwrap();
    let rest_opt = path_split.next();
    (comp, rest_opt)
}

fn rsplit_path(path: &str) -> (&str, Option<&str>) {
    let mut path_split = path.trim_matches('/').rsplitn(2, "/");
    let comp = path_split.next().unwrap();
    let rest_opt = path_split.next();
    (comp, rest_opt)
}

fn valid_long_name(mut name: &str) -> Result<()> {
    name = name.trim();
    if name.len() == 0 {
        return Err(Error::new(ErrorKind::Other, "Empty name"));
    }
    if name.len() > 255 {
        return Err(Error::new(ErrorKind::Other, "Filename too long"));
    }

    for c in name.chars() {
        match c {
            'a'...'z' | 'A'...'Z' | '0'...'9' => {},
            '\u{80}'...'\u{ffff}' => {},
            '$' |'%' | '\''| '-' | '_' | '@' | '~' | '`' | '!' | '(' | ')' | '{' | '}' | '^'
            | '#' | '&' => {},
            '+' | ',' | ';' | '=' | '[' | ']' => {},
            _ => return Err(Error::new(ErrorKind::Other, "Filename contains invalid chars"))
        }
    }
    Ok(())

}

/// https://en.wikipedia.org/wiki/8.3_filename
#[derive(Debug, Default, Clone)]
pub struct ShortNameGen {
    name: [u8; 11],
    is_lossy: bool,
    basename_len: u8,
    checksum_bitmask: u16,
    checksum: u16,
    suffix_bitmask: u16,
    name_fits: bool,
    exact_match: bool,
    is_dot: bool,
    is_dotdot: bool
}

/// Adapted from rust-fatfs: https://github.com/rafalh/rust-fatfs
impl ShortNameGen {

    const FNAME_LEN: usize = 8;
    pub fn new(mut name: &str) -> Self {
        name = name.trim();
        let mut short_name = [0x20u8; 11];
        if name == "." {
            short_name[0] = '.' as u8;
        }
        if name == ".." {
            short_name[0] = '.' as u8;
            short_name[1] = '.' as u8;
        }

        let (name_fits, basename_len, is_lossy) = match name.rfind('.') {
            Some(idx) => {
                let (b_len, fits, b_lossy) = Self::copy_part(&mut short_name[..Self::FNAME_LEN], &name[..idx]);
                let (ext_len, ext_fits, ext_lossy) = Self::copy_part(&mut short_name[Self::FNAME_LEN..Self::FNAME_LEN + 3], &name[idx + 1..]);
                (fits && ext_fits, b_len, b_lossy || ext_lossy)
            },
            None => {
                let (b_len, fits, b_lossy) = Self::copy_part(&mut short_name[..Self::FNAME_LEN], &name);
                (fits, b_len, b_lossy)
            }
        };
        let checksum = Self::checksum(name);
        ShortNameGen {

            name: short_name,
            is_lossy: is_lossy,
            is_dot: name == ".",
            is_dotdot: name == "..",
            basename_len: basename_len,
            name_fits: name_fits,
            ..Default::default()
        }


    }

    fn copy_part(dest: &mut [u8], src: &str) -> (u8, bool, bool) {
        let mut dest_len: usize = 0;
        let mut lossy_conv = false;
        for c in src.chars() {
            if dest_len == dest.len() {
                return (dest_len as u8, false, lossy_conv)
            }

            if c == ' ' || c == '.' {
                lossy_conv = true;
                continue;
            }

            let cp = match c {
                'a'...'z' | 'A'...'Z' | '0'...'9' => c,
                '$' |'%' | '\''| '-' | '_' | '@' | '~' | '`' | '!' | '(' | ')' | '{' | '}' | '^'
                | '#' | '&' => c,
                _ => '_'
            };
            lossy_conv = lossy_conv || c != cp;
            let upper =  c.to_ascii_uppercase();
            dest[dest_len] = upper as u8;
        }
        (dest_len as u8, true, lossy_conv)
    }

    // Fletcher-16 Checksum
    fn checksum(name: &str) -> u16 {
        let mut sum1: u16 = 0;
        let mut sum2: u16 = 0;
        for c in name.chars() {
            sum1 = (sum1 + (c as u16)) % 0xff;
            sum2 = (sum2 + sum1) % 0xff;
        }
        (sum2 << 8) | sum1
    }

    // Update state of generator
    // Needed in case LFNs are not present
    fn add_name(&mut self, name: &[u8; 11]) {
        // check for exact match collision
        if name == &self.name {
            self.exact_match = true;
        }

        // check for long prefix form collision (TEXTFI~1.TXT)
        let prefix_len = min(self.basename_len, 6) as usize;
        let num_suffix = if name[prefix_len] as char == '~' {
            (name[prefix_len + 1] as char).to_digit(10)
        } else {
            None
        };
        let ext_matches = name[8..] == self.name[8..];
        if name[..prefix_len] == self.name[..prefix_len] && num_suffix.is_some() && ext_matches {
            let num = num_suffix.unwrap();
            self.suffix_bitmask |= 1 << num;
        }

        // check for short prefix + checksum form collision (TE021F~1.TXT)
        let prefix_len = min(self.basename_len, 2) as usize;
        let num_suffix = if name[prefix_len + 4] as char == '~' {
            (name[prefix_len + 4 + 1] as char).to_digit(10)
        } else {
            None
        };
        if name[..prefix_len] == self.name[..prefix_len] && num_suffix.is_some() && ext_matches {
            let chksum_res = str::from_utf8(&name[prefix_len..prefix_len + 4]).map(|s| u16::from_str_radix(s, 16));
            if chksum_res == Ok(Ok(self.checksum)) {
                let num = num_suffix.unwrap(); // SAFE
                self.checksum_bitmask |= 1 << num;
            }
        }

    }

    fn generate(&self) -> Result<[u8; 11]> {
        if self.is_dot || self.is_dotdot {
            return Ok(self.name)
        }

        if !self.is_lossy && self.name_fits && !self.exact_match {
            // If there was no lossy conversion and name fits into
            // 8.3 convention and there is no collision return it as is
            return Ok(self.name);
        }
        // Try using long 6-characters prefix
        for i in 1..5 {
            if self.suffix_bitmask & (1 << i) == 0 {
                return Ok(self.build_prefixed_name(i as u32, false));
            }
        }
        // Try prefix with checksum
        for i in 1..10 {
            if self.checksum_bitmask & (1 << i) == 0 {
                return Ok(self.build_prefixed_name(i as u32, true));
            }
        }
        // Too many collisions - fail
        Err(Error::new(ErrorKind::AlreadyExists, "short name already exists"))
    }

    fn next_iteration(&mut self) {
        // Try different checksum in next iteration
        self.checksum = (num::Wrapping(self.checksum) + num::Wrapping(1)).0;
        // Zero bitmaps
        self.suffix_bitmask = 0;
        self.checksum_bitmask = 0;
    }

    fn build_prefixed_name(&self, num: u32, with_chksum: bool) -> [u8; 11] {
        let mut buf = [0x20u8; 11];
        let prefix_len = if with_chksum {
            let prefix_len = min(self.basename_len as usize, 2);
            buf[..prefix_len].copy_from_slice(&self.name[..prefix_len]);
            buf[prefix_len..prefix_len + 4].copy_from_slice(&Self::u16_to_u8_array(self.checksum));
            prefix_len + 4
        } else {
            let prefix_len = min(self.basename_len as usize, 6);
            buf[..prefix_len].copy_from_slice(&self.name[..prefix_len]);
            prefix_len
        };
        buf[prefix_len] = '~' as u8;
        buf[prefix_len + 1] = char::from_digit(num, 10).unwrap() as u8; // SAFE
        buf[8..].copy_from_slice(&self.name[8..]);
        buf
    }

    fn u16_to_u8_array(x: u16) -> [u8; 4] {
        let c1 = char::from_digit((x as u32 >> 12) & 0xF, 16).unwrap().to_ascii_uppercase() as u8;
        let c2 = char::from_digit((x as u32 >> 8) & 0xF, 16).unwrap().to_ascii_uppercase() as u8;
        let c3 = char::from_digit((x as u32 >> 4) & 0xF, 16).unwrap().to_ascii_uppercase() as u8;
        let c4 = char::from_digit((x as u32 >> 0) & 0xF, 16).unwrap().to_ascii_uppercase() as u8;
        return [c1, c2, c3, c4];
    }

}


struct LongNameEntryGenerator {
    name: Vec<u16>,
    checksum: u8,
    idx: u8,
    last_idx: u8
}

impl LongNameEntryGenerator {
    pub fn new(name: &str, checksum: u8) -> Self {
        let mut n: Vec<u16> = name.chars().map(|c| c as u16).collect();
        let pad_bytes = (13 - (n.len() % 13)) % 13;
        for i in 0..pad_bytes {
            if i == 0 {
                n.push(0);
            }
            else {
                n.push(0xffff);
            }
        }
        let start_idx = (n.len() / 13) as u8;
        LongNameEntryGenerator {
            name: n,
            checksum: checksum,
            idx: start_idx,
            last_idx: start_idx
        }
    }

    pub fn num_entries(&self) -> u8 {
        self.last_idx
    }


}

impl Iterator for LongNameEntryGenerator {
    type Item = LongDirEntry;
    fn next(&mut self) -> Option<Self::Item> {
        match self.idx {
            0 => None,
            n if n == self.last_idx => {
                let ord = n | 0x40;
                let start_idx = ((n - 1) * 13) as usize;
                self.idx -= 1;
                Some(LongDirEntry::new(ord, &self.name.as_slice()[start_idx..start_idx+13], self.checksum))
            },
            n => {
                let start_idx = ((n - 1) * 13) as usize;
                self.idx -= 1;
                Some(LongDirEntry::new(n, &self.name.as_slice()[start_idx..start_idx+13], self.checksum))
            }
        }
    }
}
/*
impl fmt::Debug for DirEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DirEntry::File(fi) =>
                write!(f, "File {{
                    file : {:?}
                  }}", fi),impl DirEntry
            DirEntry::Dir(d) =>
                write!(f, "Dir {{
                    dir : {:?}
                  }}", d)
        }
    }
}*/
