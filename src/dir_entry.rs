use std::io::{Read, Write, Seek, SeekFrom};
use std::iter::{Iterator, FromIterator};
use std::io::{ErrorKind, Error};
use std::{num, fmt, str};
use std::cmp::min;

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};

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

    /// Taken from rust-fatfs: https://github.com/debugrafalh/rust-fatfs
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

impl ShortNameGen {

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
