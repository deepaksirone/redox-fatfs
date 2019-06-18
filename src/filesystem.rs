use super::Result;

use std::io::{Read, Write, Seek, SeekFrom};
use std::path::Path;
use std::default::Default;
use std::iter::Iterator;

use BiosParameterBlock;
use disk::Disk;
use bpb::FATType;
use byteorder::{LittleEndian, ByteOrder};

struct Cluster {
    cluster_number: u64,
    parent_cluster: Option<u64>,
}

struct ClusterIter<'a, D: Read + Write + Seek> {
    current_cluster: Cluster,
    fat_start_sector: u64,
    bytes_per_sec: u64,
    fat_type: FATType,
    fs: &'a mut FileSystem<D>
}

impl<'a, D: Read + Write + Seek> Iterator for ClusterIter<'a, D> {
    type Item = Cluster;
    fn next(&mut self) -> Option<Self::Item> {
        let current_cluster = self.current_cluster.cluster_number;
        let fat_offset = match self.fat_type {
            FATType::FAT12(_) => current_cluster + (current_cluster / 2),
            FATType::FAT16(_) => current_cluster * 2,
            FATType::FAT32(_) => current_cluster * 4,
        };

        let fat_sec_number = self.fat_start_sector + (fat_offset / self.bytes_per_sec);
        let fat_ent_offset = fat_offset % self.bytes_per_sec;
        let mut sectors: [u8; 8192] = [0; 2 * 4096];
        self.fs.read_at(fat_sec_number * self.bytes_per_sec, &mut sectors[..((self.bytes_per_sec * 2) as usize)]);
        match self.fat_type {
            FATType::FAT12(_) => {
                let mut entry = LittleEndian::read_u16(&sectors[fat_ent_offset as usize ..(fat_ent_offset + 2) as usize]);
                entry = if entry & 0x0001 == 1 { entry >> 4 }
                        else { entry & 0x0fff };
                // 0x0ff7 is the bad cluster mark and any cluster value >= 0x0ff8 means EOF
                if entry >= 0x0ff7 {
                    None
                }
                else {
                    Some(Cluster {
                        cluster_number: entry as u64,
                        parent_cluster: Some(current_cluster)
                    })
                }
            },
            FATType::FAT16(_) => {

                let entry = LittleEndian::read_u16(&sectors[fat_ent_offset as usize ..(fat_ent_offset + 2) as usize]);
                // 0xfff7 is the bad cluster mark and any cluster value >= 0x0ff8 means EOF
                if (entry >= 0xfff7) {
                    None
                }
                else {
                    Some(Cluster {
                        cluster_number: entry as u64,
                        parent_cluster: Some(current_cluster)
                    })
                }
            },
            FATType::FAT32(_) => {
                let entry = LittleEndian::read_u32(&sectors[fat_ent_offset as usize ..(fat_ent_offset + 4) as usize]) & 0x0FFFFFFF;
                // 0x0ffffff7 is the bad cluster mark and any cluster value >= 0x0ffffff8 means EOF
                if (entry >= 0x0FFFFFF7) {
                    None
                }
                else {
                    Some(Cluster {
                        cluster_number: entry as u64,
                        parent_cluster: Some(current_cluster)
                    })
                }
            }
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
        Ok(0)
    }

    pub fn clusters(&mut self, start_cluster: Cluster) -> Vec<Cluster> {
        unimplemented!()
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<()> {
        self.disk.seek(SeekFrom::Start(self.partition_offset + offset))?;
        self.disk.read(buf)?;
        Ok(())
    }

    fn cluster_iter<'a>(&'a mut self, start_cluster: Cluster) -> ClusterIter<'a, D> {
        ClusterIter {
            current_cluster: start_cluster,
            fat_start_sector: self.bpb.rsvd_sec_cnt as u64,
            bytes_per_sec: self.bpb.bytes_per_sector as u64,
            fat_type: self.bpb.fat_type,
            fs: self
        }
    }

}


impl Default for Cluster {
    fn default() -> Self {
        Cluster {
            cluster_number: 0,
            parent_cluster: None
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