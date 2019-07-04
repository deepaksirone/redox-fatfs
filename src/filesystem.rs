use super::Result;

use std::io::{Read, Write, Seek, SeekFrom, Error, ErrorKind};
use std::path::Path;
use std::default::Default;
use std::iter::Iterator;
use std::cell::{RefCell};
use std::cmp::{Eq, PartialEq, Ord, PartialOrd, Ordering};

use BiosParameterBlock;
use disk::Disk;
use bpb::FATType;
use table::{FatEntry, get_entry};
use byteorder::{LittleEndian, ByteOrder, ReadBytesExt, WriteBytesExt};
use file::Dir;

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

struct Sector {
    number: u64
}

#[derive(Default, Debug, Copy, Clone)]
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
    dirty: bool
}

impl FsInfo {
    const LEAD_SIG: u32 = 0x41615252;
    const STRUC_SIG: u32 = 0x61417272;
    const TRAIL_SIG: u32 = 0xAA550000;

    fn is_valid(&self) -> bool {
        self.lead_sig == Self::LEAD_SIG && self.struc_sig == Self::STRUC_SIG &&
            self.trail_sig == Self::TRAIL_SIG
    }

    pub fn populate<D: Read + Seek>(disk: &mut D, offset: u64) -> Result<Self> {

        let mut fsinfo = FsInfo::default();
        fsinfo.lead_sig = disk.read_u32::<LittleEndian>()?;
        disk.seek(SeekFrom::Current(480))?;
        fsinfo.struc_sig = disk.read_u32::<LittleEndian>()?;
        fsinfo.free_count = disk.read_u32::<LittleEndian>()?;
        fsinfo.next_free = disk.read_u32::<LittleEndian>()?;
        disk.seek(SeekFrom::Current(12))?;
        fsinfo.trail_sig = disk.read_u32::<LittleEndian>()?;
        fsinfo.dirty = false;

        if fsinfo.is_valid() {
            Ok(fsinfo)
        }
        else {
            Err(Error::new(ErrorKind::InvalidData, "Error Parsing FsInfo"))
        }
    }

    pub fn update<D: Read + Seek>(&mut self, disk: &mut D, offset: u64) -> Result<()> {
        disk.seek(SeekFrom::Start(offset))?;
        self.lead_sig = disk.read_u32::<LittleEndian>()?;
        disk.seek(SeekFrom::Current(480))?;
        self.struc_sig = disk.read_u32::<LittleEndian>()?;
        self.free_count = disk.read_u32::<LittleEndian>()?;
        self.next_free = disk.read_u32::<LittleEndian>()?;
        disk.seek(SeekFrom::Current(12))?;
        self.trail_sig = disk.read_u32::<LittleEndian>()?;
        Ok(())
    }

    pub fn flush<D: Write + Seek>(&self, disk: &mut D, offset: u64) -> Result<()> {
        disk.seek(SeekFrom::Start(offset))?;
        disk.write_u32::<LittleEndian>(self.lead_sig)?;
        disk.seek(SeekFrom::Current(480))?;
        disk.write_u32::<LittleEndian>(self.struc_sig)?;
        disk.write_u32::<LittleEndian>(self.free_count)?;
        disk.write_u32::<LittleEndian>(self.next_free)?;
        disk.seek(SeekFrom::Current(12))?;
        disk.write_u32::<LittleEndian>(self.trail_sig)?;
        Ok(())
    }

    pub fn get_next_free(&self) -> u64 {
        self.next_free as u64
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
        disk.seek(SeekFrom::Start(partition_offset))?;
        let bpb = BiosParameterBlock::populate(&mut disk)?;

        let fsinfo = match bpb.fat_type {
            FATType::FAT32(s) => {
                let offset = partition_offset + s.fs_info as u64 * bpb.bytes_per_sector as u64;
                FsInfo::populate(&mut disk, offset).expect("Error Parsing FsInfo")
            },
            _ => FsInfo::default()
        };


        let root_dir_sec = ((bpb.root_entries_cnt as u64 * 32) + (bpb.bytes_per_sector as u64 - 1)) / (bpb.bytes_per_sector as u64);
        let fat_sz = if bpb.fat_size_16 != 0 { bpb.fat_size_16 as u64}
        else {
            match bpb.fat_type {
                FATType::FAT32(x) => x.fat_size as u64,
                _ => panic!("FAT12 and FAT16 volumes should have non-zero BPB_FATSz16")
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
        let first_sec_cluster = (cluster.cluster_number - 2) * self.sectors_per_cluster() + self.first_data_sec;
        println!("Read Cluster Offset = {:x}", first_sec_cluster * self.bytes_per_sec());
        self.read_at(first_sec_cluster * self.bytes_per_sec(), buf)
    }

    pub fn read_sector(&mut self, sector: u64, buf: &mut [u8]) -> Result<usize> {
        self.read_at(sector * self.bytes_per_sec(), buf)
    }

    pub fn clusters(&mut self, start_cluster: Cluster) -> Vec<Cluster> {
        self.cluster_iter(start_cluster).collect()
    }

    pub fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        self.disk.borrow_mut().seek(SeekFrom::Start(self.partition_offset + offset))?;
        self.disk.borrow_mut().read(buf)?;
        Ok(0)
    }

    pub fn seek_to(&mut self, offset: u64) -> Result<usize> {
        self.disk.borrow_mut().seek(SeekFrom::Start(self.partition_offset + offset))?;
        Ok(0)
    }

    pub fn write_to(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        self.disk.borrow_mut().seek(SeekFrom::Start(self.partition_offset + offset))?;
        self.disk.borrow_mut().write(buf)?;
        Ok(0)
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

    pub fn bytes_per_sec(&self) -> u64 {
        self.bpb.bytes_per_sector as u64
    }

    pub fn sectors_per_cluster(&self) -> u64 {
        self.bpb.sectors_per_cluster as u64
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
            FATType::FAT32(_) => {
                let tot_clusters = (self.bpb.total_sectors_32 as u64 + self.bpb.sectors_per_cluster as u64 - 1) / self.bpb.sectors_per_cluster as u64;
                Cluster::new(tot_clusters)
            },
            _ => {
                let tot_clusters = (self.bpb.total_sectors_16 as u64 + self.bpb.sectors_per_cluster as u64 - 1) / self.bpb.sectors_per_cluster as u64;
                Cluster::new(tot_clusters as u64)
            }
        }
    }

    fn cluster_iter(&mut self, start_cluster: Cluster) -> ClusterIter<D> {
        ClusterIter {
            current_cluster: Some(start_cluster),
            fs: self
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

impl Default for Sector {
    fn default() -> Self {
        Sector {
            number: 0
        }
    }
}