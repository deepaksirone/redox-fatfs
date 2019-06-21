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

#[derive(Copy, Clone)]
pub struct Cluster {
    pub cluster_number: u64,
    pub parent_cluster: u64,
}

struct ClusterIter<'a, D: Read + Write + Seek> {
    current_cluster: Cluster,
    fat_table: Fat,
    fs: &'a mut FileSystem<D>
}

impl<'a, D: Read + Write + Seek> Iterator for ClusterIter<'a, D> {
    type Item = Cluster;
    fn next(&mut self) -> Option<Self::Item> {
        match self.fat_table.get_entry(self.fs, self.current_cluster) {
            FatEntry::Next(c) => Some(c),
            _ => None
        }
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
        self.read_at(cluster.cluster_number, buf)
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
            current_cluster: start_cluster,
            fat_table: Fat {
                fat_type: self.bpb.fat_type
            },
            fs: self
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