use Cluster;
use filesystem::FileSystem;
use std::io::{Read, Write, Seek, SeekFrom};
use std::iter::{Iterator, FromIterator};
use std::io::{ErrorKind, Error};
use std::fmt;


use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use table::{FatEntry, get_entry};

use super::Result;

pub const DIR_ENTRY_LEN: u64 = 32;

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

    // FIXME: Add pointer to directory entry
}

#[derive(Debug, Default, Clone)]
pub struct Dir {
    pub first_cluster: Cluster,
    pub dir_path: String,
    pub dir_name: String,
    pub short_dir_entry: ShortDirEntry
}

impl Dir {
    pub fn to_iter<'a, D: Read + Write + Seek>(&self, fs: &'a mut FileSystem<D>) -> DirIter<'a, D> {
        DirIter {
            current_cluster: self.first_cluster,
            dir_path: self.dir_path.clone(),
            offset: 0,
            fs: fs
        }
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
    name1: [u8; 10],
    /// File Attributes
    file_attrs: FileAttributes,
    /// Entry Type: If zero indicates that the entry
    /// is a subcomponent of a long name
    /// Non-zero values are reserved
    dirent_type: u8,
    /// Checksum computed from short name
    chksum: u8,
    /// Characters 6-11 of name
    name2: [u8; 10],
    /// FirstCluster Low Word
    /// Should be zero in a long file entry
    first_clus_low: u16,
    /// Characters 12-13 of name
    name3: [u8; 4]
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

    pub fn to_dir_entry(&self, dir_path: &String) -> DirEntry{
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
            dir.short_dir_entry = self.clone();
            DirEntry::Dir(dir)
        }

    }

    fn compute_checksum(&self) -> u8 {
        let mut sum = 0;
        for b in &self.dir_name {
            sum = if (sum & 1) > 0 { 0x80 } else { 0 } + (sum >> 1) + b;
        }
        sum
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

pub struct DirIter<'a, D: Read + Write + Seek> {
    current_cluster: Cluster,
    dir_path: String,
    offset: u64,
    fs: &'a mut FileSystem<D>
}

impl<'a, D: Read + Write + Seek> Iterator for DirIter<'a, D> {
    type Item = DirEntry;
    fn next(&mut self) -> Option<Self::Item> {
        if self.offset >= self.fs.bytes_per_cluster() {
            match get_entry(self.fs, self.current_cluster).ok() {
                Some(FatEntry::Next(c)) => {
                    self.current_cluster = c;
                    self.offset = self.offset % self.fs.bytes_per_cluster();
                },
                _ => return None
            }
        }

        let dir_entry_raw = get_dir_entry_raw(self.fs, self.offset).ok();
        /*match dir_entry_raw {
            Some(DirEntryRaw::Short(s)) => {


            }
        }*/
        None
    }
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
                fs.disk.borrow_mut().read(&mut ldr.name1)?;
                ldr.file_attrs = FileAttributes::from_bits(fs.disk.borrow_mut().read_u8()?)
                    .ok_or(Error::new(ErrorKind::Other, "Error Reading File Attr"))?;
                ldr.dirent_type = fs.disk.borrow_mut().read_u8()?;
                ldr.chksum = fs.disk.borrow_mut().read_u8()?;
                fs.disk.borrow_mut().read(&mut ldr.name2)?;
                ldr.first_clus_low = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
                fs.disk.borrow_mut().read(&mut ldr.name3)?;
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
