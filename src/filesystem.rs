use super::Result;
use BLOCK_SIZE;

use std::io::{Read, Write, Seek, SeekFrom, Error, ErrorKind, Cursor};
use std::default::Default;
use std::iter::Iterator;
use std::cell::{RefCell};
use std::cmp::{Eq, PartialEq, PartialOrd, Ordering, min};

use BiosParameterBlock;
//use disk::Disk;
use bpb::FATType;
use table::{FatEntry, get_entry, get_entry_raw, set_entry, RESERVED_CLUSTERS};
use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use dir_entry::Dir;

#[derive(Copy, Clone, Debug)]
pub struct Cluster {
    pub cluster_number: u64,
    pub parent_cluster: u64,
}

impl PartialOrd for Cluster {
    fn partial_cmp(&self, other: &Cluster) -> Option<Ordering> {
        self.cluster_number.partial_cmp(&other.cluster_number)
    }
}

impl PartialEq for Cluster {
    fn eq(&self, other: &Self) -> bool {
        self.cluster_number == other.cluster_number
    }
}

impl Eq for Cluster {}

struct ClusterIter<'a, D: Read + Write + Seek> {
    current_cluster: Option<Cluster>,
    fs: &'a mut FileSystem<D>
}

impl<'a, D: Read + Write + Seek> Iterator for ClusterIter<'a, D> {
    type Item = Cluster;
    fn next(&mut self) -> Option<Self::Item> {
        let ret = self.current_cluster;
        let new = match self.current_cluster {
            Some(c) => {
                let entry = get_entry(self.fs, c).ok();
                match entry {
                    Some(FatEntry::Next(c)) => {
                        Some(c)
                    },
                    _ => None
                }
            },
            _ => None
        };
        self.current_cluster = new;
        ret
    }
}


/// An in-memory copy of FsInfo Struct for FAT32
/// Flushed out to disk on unmounting the volume
#[derive(Debug, Copy, Clone)]
pub struct FsInfo {
    /// Lead Signature - must equal 0x41615252
    lead_sig: u32,
    /// Value must equal 0x61417272
    struc_sig: u32,
    /// Last known free cluster count
    free_count: u32,
    /// Hint for free cluster locations
    next_free: u32,
    /// 0xAA550000
    trail_sig: u32,
    /// Dirty flag to flush to disk
    dirty: bool,
    /// Relative Offset of FsInfo Structure
    /// Not present for FAT12 and FAT16
    offset: Option<u64>
}

impl FsInfo {
    const LEAD_SIG: u32 = 0x41615252;
    const STRUC_SIG: u32 = 0x61417272;
    const TRAIL_SIG: u32 = 0xAA550000;
    const FS_INFO_SIZE: u64 = 512;

    fn is_valid(&self) -> bool {
        self.lead_sig == Self::LEAD_SIG && self.struc_sig == Self::STRUC_SIG &&
            self.trail_sig == Self::TRAIL_SIG
    }

    pub fn populate<D: Read + Seek>(disk: &mut D, offset: u64) -> Result<Self> {
        let block_vec = get_block_buffer(offset, Self::FS_INFO_SIZE);
        let mut cursor = Cursor::new(block_vec);
        let mut fsinfo = FsInfo::default();

        disk.seek(SeekFrom::Start((offset / BLOCK_SIZE) * BLOCK_SIZE));
        let read = disk.read(cursor.get_mut())?;
        println!("Read {:?} bytes into block vec", read);
        cursor.seek(SeekFrom::Start(offset % BLOCK_SIZE))?;
        println!("Seeking cursor to offset: {:?}", offset % BLOCK_SIZE);

        fsinfo.lead_sig = cursor.read_u32::<LittleEndian>()?;
        cursor.seek(SeekFrom::Current(480))?;
        fsinfo.struc_sig = cursor.read_u32::<LittleEndian>()?;
        fsinfo.free_count = cursor.read_u32::<LittleEndian>()?;
        fsinfo.next_free = cursor.read_u32::<LittleEndian>()?;
        cursor.seek(SeekFrom::Current(12))?;
        fsinfo.trail_sig = cursor.read_u32::<LittleEndian>()?;
        fsinfo.dirty = false;
        fsinfo.offset = Some(offset);

        if fsinfo.is_valid() {
            Ok(fsinfo)
        }
        else {
            Err(Error::new(ErrorKind::InvalidData, "Error Parsing FsInfo"))
        }
    }

    pub fn update<D: Read + Seek>(&mut self, disk: &mut D) -> Result<()> {
        if let Some(off) = self.offset {
            let block_vec = get_block_buffer(off, Self::FS_INFO_SIZE);
            let mut cursor = Cursor::new(block_vec);
            disk.seek(SeekFrom::Start((off / BLOCK_SIZE) * BLOCK_SIZE))?;
            disk.read(cursor.get_mut())?;
            cursor.seek(SeekFrom::Start(off % BLOCK_SIZE))?;

            self.lead_sig = cursor.read_u32::<LittleEndian>()?;
            cursor.seek(SeekFrom::Current(480))?;
            self.struc_sig = cursor.read_u32::<LittleEndian>()?;
            self.free_count = cursor.read_u32::<LittleEndian>()?;
            self.next_free = cursor.read_u32::<LittleEndian>()?;
            cursor.seek(SeekFrom::Current(12))?;
            self.trail_sig = cursor.read_u32::<LittleEndian>()?;
        }
        Ok(())
    }

    pub fn flush<D: Read + Write + Seek>(&self, disk: &mut D) -> Result<()> {
        if let Some(off) = self.offset {
            let block_vec = get_block_buffer(off, Self::FS_INFO_SIZE);
            let mut cursor = Cursor::new(block_vec);
            disk.seek(SeekFrom::Start((off / BLOCK_SIZE) * BLOCK_SIZE))?;
            disk.read(cursor.get_mut())?;
            cursor.seek(SeekFrom::Start(off % BLOCK_SIZE))?;

            cursor.write_u32::<LittleEndian>(self.lead_sig)?;
            cursor.seek(SeekFrom::Current(480))?;
            cursor.write_u32::<LittleEndian>(self.struc_sig)?;
            cursor.write_u32::<LittleEndian>(self.free_count)?;
            cursor.write_u32::<LittleEndian>(self.next_free)?;
            cursor.seek(SeekFrom::Current(12))?;
            cursor.write_u32::<LittleEndian>(self.trail_sig)?;

            disk.seek(SeekFrom::Start((off / BLOCK_SIZE) * BLOCK_SIZE))?;
            disk.write(cursor.get_ref())?;
        }
        Ok(())
    }

    pub fn get_next_free(&self) -> Option<u64> {
        match self.next_free {
            0xFFFFFFFF => None,
            0 | 1 => None,
            n => Some(n as u64)
        }
    }

    pub fn get_free_count(&self, max_cluster: Cluster) -> Option<u64> {
        let count_clusters = max_cluster.cluster_number - RESERVED_CLUSTERS + 1;
        if self.free_count as u64 > count_clusters {
            None
        }
        else {
            match self.free_count {
                0xFFFFFFFF => None,
                n => Some(n as u64)
            }
        }
    }

    pub fn update_free_count(&mut self, count: u64) {
        self.free_count = count as u32;
    }

    pub fn delta_free_count(&mut self, delta: i32) {
        self.free_count = (self.free_count as i32 + delta) as u32;
    }

    pub fn update_next_free(&mut self, next_free: u64) {
        self.next_free = next_free as u32;
    }

}

impl Default for FsInfo {
    fn default() -> Self {
        FsInfo {
            lead_sig: 0x41615252,
            struc_sig: 0x61417272,
            free_count: 0xFFFFFFFF,
            next_free: RESERVED_CLUSTERS as u32,
            trail_sig: 0xAA550000,
            dirty: false,
            offset: None
        }
    }
}
pub struct FileSystem<D: Read + Write + Seek> {
    pub disk: RefCell<D>,
    pub bpb: BiosParameterBlock,
    pub partition_offset: u64,
    pub first_data_sec: u64,
    pub fs_info: RefCell<FsInfo>
}

impl<D: Read + Write + Seek> FileSystem<D> {

    pub fn from_offset(partition_offset: u64, mut disk: D) -> Result<FileSystem<D>> {
        disk.seek(SeekFrom::Start((partition_offset / BLOCK_SIZE) * BLOCK_SIZE))?;
        let bpb = BiosParameterBlock::populate(&mut disk)?;

        let fsinfo = match bpb.fat_type {
            FATType::FAT32(s) => {
                let offset = partition_offset + s.fs_info as u64 * bpb.bytes_per_sector as u64;
                FsInfo::populate(&mut disk, offset)?
            },
            _ => FsInfo::default()
        };


        let root_dir_sec = ((bpb.root_entries_cnt as u64 * 32) + (bpb.bytes_per_sector as u64 - 1)) / (bpb.bytes_per_sector as u64);
        let fat_sz = if bpb.fat_size_16 != 0 { bpb.fat_size_16 as u64}
        else {
            match bpb.fat_type {
                FATType::FAT32(x) => x.fat_size as u64,
                _ => return Err(Error::new(ErrorKind::InvalidData, "FAT12 and FAT16 volumes should have non-zero BPB_FATSz16"))
            }
        };
        let first_data_sec = bpb.rsvd_sec_cnt as u64 + (bpb.num_fats as u64 * fat_sz) + root_dir_sec;

        Ok(FileSystem {
            disk: RefCell::new(disk),
            bpb: bpb,
            partition_offset: partition_offset,
            first_data_sec: first_data_sec,
            fs_info: RefCell::new(fsinfo)
        })
    }

    pub fn read_cluster(&mut self, cluster: Cluster, buf: &mut [u8]) -> Result<usize> {
        /*let root_dir_sec = ((self.bpb.root_entries_cnt as u64 * 32) + (self.bpb.bytes_per_sector as u64 - 1)) / (self.bpb.bytes_per_sector as u64);
        let fat_sz = if self.bpb.fat_size_16 != 0 { self.bpb.fat_size_16 as u64}
                         else {
                            match self.bpb.fat_type {
                                FATType::FAT32(x) => x.fat_size as u64,
                                _ => panic!("FAT12 and FAT16 volumes should have non-zero BPB_FATSz16")
                            }
                         };
        let first_data_sec = self.bpb.rsvd_sec_cnt as u64 + (self.bpb.num_fats as u64 * fat_sz) + root_dir_sec;*/
        let bytes_per_sec = self.bytes_per_sec();
        let first_sec_cluster = (cluster.cluster_number - 2) * self.sectors_per_cluster() + self.first_data_sec;
        println!("Read Cluster Offset = {:x}", first_sec_cluster * self.bytes_per_sec());
        self.read_at(first_sec_cluster * bytes_per_sec, buf)
    }

    pub fn read_sector(&mut self, sector: u64, buf: &mut [u8]) -> Result<usize> {
        let bytes_per_sec = self.bytes_per_sec();
        self.read_at(sector * bytes_per_sec, buf)
    }

    pub fn clusters(&mut self, start_cluster: Cluster) -> Vec<Cluster> {
        self.cluster_iter(start_cluster).collect()
    }

    pub fn num_clusters_chain(&mut self, start_cluster: Cluster) -> u64 {
        self.cluster_iter(start_cluster).fold(0, |sz, _cluster| sz + 1)
    }

    pub fn read_at(&mut self, mut offset: u64, buf: &mut [u8]) -> Result<usize> {
        //let partition_offset = self.partition_offset;
        //self.disk.borrow_mut().seek(SeekFrom::Start(partition_offset + offset))?;
        //self.disk.borrow_mut().read(buf)
        let num_blocks = (buf.len() + BLOCK_SIZE as usize - 1) / BLOCK_SIZE as usize;
        let blk_offset = self.get_block_offset(offset);

        let block_buf = get_block_buffer(self.get_raw_offset(offset), BLOCK_SIZE);
        let mut cursor = Cursor::new(block_buf);
        let mut start = 0;

        for i in 0..num_blocks {
            self.seek_to_block(offset)?;
            self.disk.borrow_mut().read_exact(cursor.get_mut())?;
            cursor.seek(SeekFrom::Start(blk_offset))?;
            let bytes_remaining_block = BLOCK_SIZE - blk_offset;
            let read_len = min(bytes_remaining_block as usize, buf.len() - start);
            cursor.read(&mut buf[start.. start + read_len])?;
            start += read_len;
            offset += read_len as u64;
        }

        Ok(start)
    }

    pub fn seek_to(&mut self, offset: u64) -> Result<usize> {
        match self.disk.borrow_mut().seek(SeekFrom::Start(self.partition_offset + offset)) {
            Ok(s) => Ok(s as usize),
            Err(e) => Err(e)
        }
    }

    pub fn seek_to_block(&mut self, offset: u64) -> Result<usize> {
        let off = self.partition_offset + offset;
        let block = off / BLOCK_SIZE;
        self.disk.borrow_mut().seek(SeekFrom::Start(block * BLOCK_SIZE)).map(|s| s as usize)
    }

    pub fn get_block_offset(&self, offset: u64) -> u64 {
        (self.partition_offset + offset) % BLOCK_SIZE
    }

    pub fn get_raw_offset(&self, offset: u64) -> u64 {
        self.partition_offset + offset
    }

    pub fn write_to(&mut self, mut offset: u64, buf: &[u8]) -> Result<usize> {
        //self.disk.borrow_mut().seek(SeekFrom::Start(self.partition_offset + offset))?;
        //let written = self.disk.borrow_mut().write(buf)?;
        //self.disk.borrow_mut().flush()?;
        //println!("Write Success");
        //Ok(written)
        let num_blocks = (buf.len() + BLOCK_SIZE as usize- 1) / BLOCK_SIZE as usize;
        let blk_offset = self.get_block_offset(offset);

        let block_buf = get_block_buffer(self.get_raw_offset(offset), BLOCK_SIZE);
        let mut cursor = Cursor::new(block_buf);
        let mut start = 0;

        for i in 0..num_blocks {
            self.seek_to_block(offset)?;
            self.disk.borrow_mut().read_exact(cursor.get_mut())?;
            cursor.seek(SeekFrom::Start(blk_offset))?;

            let bytes_remaining_block = BLOCK_SIZE - blk_offset;
            let write_len = min(bytes_remaining_block as usize, buf.len() - start);
            cursor.write(&buf[start .. start + write_len])?;
            start += write_len;
            offset += write_len as u64;

            self.seek_to_block(offset)?;
            self.disk.borrow_mut().write(cursor.get_ref())?;
        }

        Ok(start)
    }

    pub fn seek_to_cluster(&mut self, cluster: Cluster) -> Result<usize> {
        let bytes_per_sec = self.bytes_per_sec();
        let first_sec_cluster = (cluster.cluster_number - 2) * self.sectors_per_cluster() + self.first_data_sec;
        self.disk.borrow_mut().seek(SeekFrom::Start(first_sec_cluster * bytes_per_sec))?;
        Ok(0)
    }

    pub fn zero_cluster(&mut self, cluster: Cluster) -> Result<()> {
        let zeroes = vec![0; self.bytes_per_cluster() as usize];
        let offset = self.cluster_offset(cluster);
        self.write_to(offset, zeroes.as_slice())?;
        Ok(())
    }

    pub fn fat_size(&self) -> u64 {
        if self.bpb.fat_size_16 != 0 { self.bpb.fat_size_16 as u64 }
        else {
            match self.bpb.fat_type {
                FATType::FAT32(x) => x.fat_size as u64,
                _ => panic!("FAT12 and FAT16 volumes should have non-zero BPB_FATSz16")
            }
        }
    }

    pub fn fat_start_sector(&self) -> u64 {
        let active_fat = self.active_fat();
        let fat_sz = self.fat_size();
        self.bpb.rsvd_sec_cnt as u64 + (active_fat * fat_sz)
    }

    #[inline]
    pub fn bytes_per_sec(&self) -> u64 {
        self.bpb.bytes_per_sector as u64
    }

    pub fn sectors_per_cluster(&self) -> u64 {
        self.bpb.sectors_per_cluster as u64
    }

    pub fn bytes_per_cluster(&self) -> u64 {
        self.bytes_per_sec() * self.sectors_per_cluster()
    }

    pub fn root_dir_offset(&self) -> u64 {
        match self.bpb.fat_type {
            FATType::FAT32(s) => {
                //let bytes_per_sec = self.bytes_per_sec();
                let first_sec_cluster = (s.root_cluster as u64 - 2) * self.sectors_per_cluster() + self.first_data_sec;
                first_sec_cluster * self.bytes_per_sec()
            },
            _ => {
                let root_sec = self.bpb.rsvd_sec_cnt as u64 + (self.bpb.num_fats as u64 * self.bpb.fat_size_16 as u64);
                root_sec * self.bytes_per_sec()
            }
        }
    }

    pub fn root_dir_end_offset(&self) -> Option<u64> {
        match self.bpb.fat_type {
            FATType::FAT16(_) | FATType::FAT12(_) => Some(self.root_dir_offset() + (self.bpb.root_entries_cnt as u64 * 32)),
            _ => None
        }
    }

    pub fn root_dir(&mut self) -> Dir {
        match self.bpb.fat_type {
            FATType::FAT32(s) => {
                Dir {
                    first_cluster: Cluster::new(s.root_cluster as u64),
                    dir_path: "/".to_string(),
                    dir_name: String::from("/"),
                    root_offset: None,
                    short_dir_entry: None,
                    loc: None
                }
            },
            _ => {
                Dir {
                    first_cluster: Cluster::new(0),
                    dir_path: "/".to_string(),
                    dir_name: String::from("/"),
                    root_offset: Some(self.root_dir_offset()),
                    short_dir_entry: None,
                    loc: None
                }
            }
        }
    }

    // Returns zero when the cluster offset makes no sense
    pub fn cluster_offset(&self, cluster: Cluster) -> u64 {
        //let bytes_per_sec = self.bytes_per_sec();
        if cluster.cluster_number >= 2 {
            let first_sec_cluster = (cluster.cluster_number - 2) * self.sectors_per_cluster() + self.first_data_sec;
            first_sec_cluster * self.bytes_per_sec()
        } else {
            0
        }
    }

    pub fn mirroring_enabled(&self) -> bool {
        match self.bpb.fat_type {
            FATType::FAT32(s) => s.ext_flags & 0x80 == 0,
            _ => false
        }
    }

    pub fn active_fat(&self) -> u64 {
        if self.mirroring_enabled() {
            0
        }
        else {
            match self.bpb.fat_type {
                FATType::FAT32(s) => (s.ext_flags & 0x0F) as u64,
                _ => 0
            }
        }
    }

    pub fn max_cluster_number(&self) -> Cluster {
        match self.bpb.fat_type {
            FATType::FAT32(s) => {
                let data_sec = self.bpb.total_sectors_32 as u64 - (self.bpb.rsvd_sec_cnt as u64 + (self.bpb.num_fats as u64 * s.fat_size as u64));
                let tot_clusters = data_sec / self.bpb.sectors_per_cluster as u64;
                Cluster::new(tot_clusters + RESERVED_CLUSTERS - 1)
            },
            _ => {
                let root_dir_sectors = ((self.bpb.root_entries_cnt as u64 * 32) + self.bytes_per_sec() - 1) / self.bytes_per_sec();
                let data_sec = self.bpb.total_sectors_16 as u64 - (self.bpb.rsvd_sec_cnt as u64 + (self.bpb.num_fats as u64 * self.bpb.fat_size_16 as u64) + root_dir_sectors);
                let tot_clusters = data_sec / self.bpb.sectors_per_cluster as u64;
                Cluster::new(tot_clusters + RESERVED_CLUSTERS - 1)
            }
        }
    }

    fn cluster_iter(&mut self, start_cluster: Cluster) -> ClusterIter<D> {
        ClusterIter {
            current_cluster: Some(start_cluster),
            fs: self
        }
    }

    pub fn get_cluster_relative(&mut self, start_cluster: Cluster, n: usize) -> Option<Cluster> {
            self.cluster_iter(start_cluster).skip(n).next()
    }

    pub fn get_last_cluster(&mut self, start_cluster: Cluster) -> Option<Cluster> {
        self.cluster_iter(start_cluster).last()
    }

    pub fn clean_shut_bit(&mut self) -> Result<bool> {
        match self.bpb.fat_type {
            FATType::FAT32(_) => {
                let bit = get_entry_raw(self, Cluster::new(1))? & 0x08000000;
                Ok(bit > 0)
            },
            FATType::FAT16(_) => {
                let bit = get_entry_raw(self, Cluster::new(1))? & 0x8000;
                Ok(bit > 0)
            },
            _ => Ok(true)
        }
    }

    pub fn hard_error_bit(&mut self) -> Result<bool> {
        match self.bpb.fat_type {
            FATType::FAT32(_) => {
                let bit = get_entry_raw(self, Cluster::new(1))? & 0x04000000;
                Ok(bit > 0)
            },
            FATType::FAT16(_) => {
                let bit = get_entry_raw(self, Cluster::new(1))? & 0x4000;
                Ok(bit > 0)
            },
            _ => Ok(true)
        }
    }

    pub fn set_clean_shut_bit(&mut self) -> Result<()> {
        match self.bpb.fat_type {
            FATType::FAT32(_) => {
                let raw_entry = get_entry_raw(self, Cluster::new(1))? | 0x08000000;
                set_entry(self, Cluster::new(1), FatEntry::Next(Cluster::new(raw_entry)))?;
                Ok(())
            },
            FATType::FAT16(_) => {
                let raw_entry = get_entry_raw(self, Cluster::new(1))? | 0x8000;
                set_entry(self, Cluster::new(1), FatEntry::Next(Cluster::new(raw_entry)))?;
                Ok(())
            },
            _ => Ok(())
        }
    }

    pub fn set_hard_error_bit(&mut self) -> Result<()> {
        match self.bpb.fat_type {
            FATType::FAT32(_) => {
                let raw_entry = get_entry_raw(self, Cluster::new(1))? | 0x04000000;
                set_entry(self, Cluster::new(1), FatEntry::Next(Cluster::new(raw_entry)))?;
                Ok(())
            },
            FATType::FAT16(_) => {
                let raw_entry = get_entry_raw(self, Cluster::new(1))? | 0x4000;
                set_entry(self, Cluster::new(1), FatEntry::Next(Cluster::new(raw_entry)))?;
                Ok(())
            },
            _ => Ok(())
        }
    }

    pub fn unmount(&mut self) -> Result<()> {
        self.fs_info.borrow_mut().flush(self.disk.get_mut())?;
        self.set_clean_shut_bit()?;
        self.set_hard_error_bit()?;
        self.disk.borrow_mut().flush()?;
        Ok(())
    }

    //pub fn flush()

}

impl<D: Read + Write + Seek> Drop for FileSystem<D> {
    fn drop(&mut self) {
        match self.unmount() {
            _ => {}
        }
    }
}

impl Cluster {
    pub fn new(cluster: u64) -> Self {
        Cluster {
            cluster_number: cluster,
            parent_cluster: 0
        }
    }
}

impl Default for Cluster {
    fn default() -> Self {
        Cluster {
            cluster_number: 0,
            parent_cluster: 0
        }
    }
}

pub fn get_block_buffer(byte_offset: u64, read_size: u64) -> Vec<u8> {
    let block_offset = byte_offset % BLOCK_SIZE;
    let tot_blocks = (block_offset + read_size + BLOCK_SIZE - 1) / BLOCK_SIZE;
    vec![0; tot_blocks as usize * BLOCK_SIZE as usize]

}