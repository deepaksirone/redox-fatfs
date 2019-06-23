use super::Result;

use std::io::{Read, Write, Seek, SeekFrom};
use std::path::Path;
use std::default::Default;
use std::iter::Iterator;

use BiosParameterBlock;
use disk::Disk;
use bpb::FATType;
use table::{Fat, FatEntry};
use byteorder::{LittleEndian, ByteOrder};
use file::Dir;

#[derive(Copy, Clone, Debug)]
pub struct Cluster {
    pub cluster_number: u64,
    pub parent_cluster: u64,
}

struct ClusterIter<'a, D: Read + Write + Seek> {
    current_cluster: Option<Cluster>,
    fat_table: Fat,
    fs: &'a mut FileSystem<D>
}

impl<'a, D: Read + Write + Seek> Iterator for ClusterIter<'a, D> {
    type Item = Cluster;
    fn next(&mut self) -> Option<Self::Item> {
        let ret = self.current_cluster;
        let new = match self.current_cluster {
            Some(c) => {
                let entry = self.fat_table.get_entry(self.fs, c);
                match entry {
                    FatEntry::Next(c) => {
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

pub struct FileSystem<D: Read + Write + Seek> {
    pub disk: D,
    pub bpb: BiosParameterBlock,
    pub partition_offset: u64,
}

impl<D: Read + Write + Seek> FileSystem<D> {

    pub fn from_offset(partition_offset: u64, mut disk: D) -> Result<FileSystem<D>> {
        disk.seek(SeekFrom::Start(partition_offset))?;
        let bpb = BiosParameterBlock::populate(&mut disk)?;


        Ok(FileSystem {
            disk: disk,
            bpb: bpb,
            partition_offset: partition_offset,
        })
    }

    pub fn read_cluster(&mut self, cluster: Cluster, buf: &mut [u8]) -> Result<usize> {
        let root_dir_sec = ((self.bpb.root_entries_cnt as u64 * 32) + (self.bpb.bytes_per_sector as u64 - 1)) / (self.bpb.bytes_per_sector as u64);
        let fat_sz = if self.bpb.fat_size_16 != 0 { self.bpb.fat_size_16 as u64}
                         else {
                            match self.bpb.fat_type {
                                FATType::FAT32(x) => x.fat_size as u64,
                                _ => panic!("FAT12 and FAT16 volumes should have non-zero BPB_FATSz16")
                            }
                         };
        let first_data_sec = self.bpb.rsvd_sec_cnt as u64 + (self.bpb.num_fats as u64 * fat_sz) + root_dir_sec;
        let first_sec_cluster = (cluster.cluster_number - 2) * (self.bpb.sectors_per_cluster as u64) + first_data_sec;
        println!("Read Cluster Offset = {:x}", first_sec_cluster * (self.bpb.bytes_per_sector as u64));
        self.read_at(first_sec_cluster * (self.bpb.bytes_per_sector as u64), buf)
    }

    pub fn clusters(&mut self, start_cluster: Cluster) -> Vec<Cluster> {
        self.cluster_iter(start_cluster).collect()
    }

    pub fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<usize> {
        self.disk.seek(SeekFrom::Start(self.partition_offset + offset))?;
        self.disk.read(buf)?;
        Ok(0)
    }

    pub fn fat_start_sector(&self) -> u64 {
        self.bpb.rsvd_sec_cnt as u64
    }

    pub fn bytes_per_sec(&self) -> u64 {
        self.bpb.bytes_per_sector as u64
    }


    fn cluster_iter(&mut self, start_cluster: Cluster) -> ClusterIter<D> {
        ClusterIter {
            current_cluster: Some(start_cluster),
            fat_table: Fat {
                fat_type: self.bpb.fat_type
            },
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