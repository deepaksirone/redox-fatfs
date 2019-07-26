use std::io::{Read, Write, Seek, SeekFrom};
use std::iter::{Iterator, FromIterator};
use std::io::{ErrorKind, Error};
use std::{num, fmt, str};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

use Cluster;
use filesystem::FileSystem;
use table::{FatEntry, get_entry};

use super::Result;

pub const DIR_ENTRY_LEN: u64 = 32;
pub const LFN_PART_LEN: usize = 13;

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

pub struct DirRange {

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

    pub fn is_root(&self) -> bool {
        self.root_offset.is_some()
    }

     //pub fn find_entry(&self, name: &str, )
    // TODO: open, create_file, create_dir, find_entry
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
        debug_assert!(name_part.len() == LFN_PART_LEN);
        name_part[0..5].copy_from_slice(&self.name1);
        name_part[5..11].copy_from_slice(&self.name2);
        name_part[11..13].copy_from_slice(&self.name3);
    }

    pub fn order(&self) -> u8 {
        self.ord
    }

    pub fn chksum(&self) -> u8 {
        self.chksum
    }
}

impl ShortDirEntry {
    const PADDING: u8 = ' ' as u8;

    pub fn is_dir(&self) -> bool {
        self.file_attrs.contains(FileAttributes::DIRECTORY)
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

    pub fn to_dir_entry(&self, loc: (Cluster, u64), dir_path: &String) -> DirEntry{
        if !self.file_attrs.contains(FileAttributes::DIRECTORY) {
            let mut file = File::default();
            let f_name = self.name_to_string();
            let mut f_path = dir_path.clone();
            f_path.push('/');
            f_path.push_str(&f_name.clone());
            let cluster = Cluster::new((self.fst_clus_lo as u64) | ((self.fst_clst_hi as u64) << 16));
            file.first_cluster = cluster;
            file.file_path = f_path;
            file.fname = f_name;
            file.short_dir_entry = self.clone();
            file.loc = (loc, loc);
            DirEntry::File(file)
        } else {
            let mut dir = Dir::default();
            let cluster = Cluster::new((self.fst_clus_lo as u64) | ((self.fst_clst_hi as u64) << 16));
            dir.first_cluster = cluster;
            let dir_name = self.name_to_string();
            let mut d_path = dir_path.clone();
            d_path.push('/');
            d_path.push_str(&dir_name.clone());
            dir.dir_path = d_path;
            dir.dir_name = dir_name;
            dir.root_offset = None;
            dir.short_dir_entry = Some(self.clone());
            dir.loc = Some((loc, loc));
            DirEntry::Dir(dir)
        }

    }

    pub fn to_dir_entry_lfn(&self, name: String, loc: ((Cluster, u64), (Cluster, u64)), dir_path: &String) -> DirEntry {
        if !self.file_attrs.contains(FileAttributes::DIRECTORY) {
            let mut file = File::default();
            let mut f_path = dir_path.clone();
            f_path.push('/');
            f_path.push_str(&name.clone());
            let cluster = Cluster::new((self.fst_clus_lo as u64) | ((self.fst_clst_hi as u64) << 16));
            file.first_cluster = cluster;
            file.file_path = f_path;
            file.fname = name;
            file.short_dir_entry = self.clone();
            file.loc = loc;
            DirEntry::File(file)
        } else {
            let mut dir = Dir::default();
            let cluster = Cluster::new((self.fst_clus_lo as u64) | ((self.fst_clst_hi as u64) << 16));
            dir.first_cluster = cluster;
            let mut d_path = dir_path.clone();
            d_path.push('/');
            d_path.push_str(&name.clone());
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
    Dir(Dir)
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

struct ShortNameGen {
    name: [u8; 11],
    is_lossy: bool,

}
/*
impl fmt::Debug for DirEntry {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            DirEntry::File(fi) =>
                write!(f, "File {{
                    file : {:?}
                  }}", fi),
            DirEntry::Dir(d) =>
                write!(f, "Dir {{
                    dir : {:?}
                  }}", d)
        }
    }
}*/
