use bpb::FATType;
use super::Result;
use std::io::{Read, Write, Seek, SeekFrom};
use filesystem::{FileSystem, Cluster, FsInfo};
use byteorder::{LittleEndian, ByteOrder, ReadBytesExt};

#[derive(Debug, Clone, Copy)]
pub struct Fat {
    pub fat_type: FATType,
}

pub enum FatEntry {
    Unused,
    Bad,
    EndOfChain,
    Next(Cluster)
}

impl Fat {
    pub fn get_entry<D: Read + Seek + Write>(&mut self, fs: &mut FileSystem<D>, cluster: Cluster) -> Result<FatEntry> {
        let current_cluster = cluster.cluster_number;
        let fat_offset = match self.fat_type {
            FATType::FAT12(_) => current_cluster + (current_cluster / 2),
            FATType::FAT16(_) => current_cluster * 2,
            FATType::FAT32(_) => current_cluster * 4,
        };

        let fat_start_sector = fs.fat_start_sector();
        let bytes_per_sec = fs.bytes_per_sec();

        let fat_sec_number = fat_start_sector + (fat_offset / bytes_per_sec);
        let fat_ent_offset = fat_offset % bytes_per_sec;
        //let mut sectors: [u8; 8192] = [0; 2 * 4096];
        //fs.read_at(fat_sec_number * bytes_per_sec, &mut sectors[..((bytes_per_sec * 2) as usize)]);
        fs.seek_to(fat_sec_number * bytes_per_sec + fat_ent_offset)?;

        let res = match self.fat_type {
            FATType::FAT12(_) => {

                let mut entry = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
                //let mut entry = LittleEndian::read_u16(&sectors[fat_ent_offset as usize ..(fat_ent_offset + 2) as usize]);
                entry = if entry & 0x0001 == 1 { entry >> 4 }
                        else { entry & 0x0fff };
                // 0x0ff7 is the bad cluster mark and any cluster value >= 0x0ff8 means EOF
                if entry == 0 {
                    FatEntry::Unused
                }
                else if entry == 0x0ff7 {
                    FatEntry::Bad
                }
                else if entry >= 0xff8 {
                    FatEntry::EndOfChain
                }
                else {
                    FatEntry::Next(Cluster {
                        cluster_number: entry as u64,
                        parent_cluster: cluster.cluster_number
                    })
                }
            }

            FATType::FAT16(_) => {
                //let mut entry = LittleEndian::read_u16(&sectors[fat_ent_offset as usize ..(fat_ent_offset + 2) as usize]);
                let mut entry = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
                if entry == 0 {
                    FatEntry::Unused
                }
                else if entry == 0xfff7 {
                    FatEntry::Bad
                }
                else if entry >= 0xfff8 {
                    FatEntry::EndOfChain
                }
                else {
                    FatEntry::Next(Cluster {
                        cluster_number: entry as u64,
                        parent_cluster: cluster.cluster_number
                    })
                }
            }

            FATType::FAT32(_) => {
                //let mut entry = LittleEndian::read_u32(&sectors[fat_ent_offset as usize ..(fat_ent_offset + 4) as usize]) & 0x0fffffff;
                let mut entry = fs.disk.borrow_mut().read_u32::<LittleEndian>()?;
                match entry {
                    n if (cluster.cluster_number >= 0x0ffffff7 && cluster.cluster_number <= 0x0fffffff) => {
                        // Handling the case where the current cluster number is not an allocatable cluster number
                        FatEntry::Bad
                    },
                    0 => FatEntry::Unused,
                    0x0ffffff7 => FatEntry::Bad,
                    0x0ffffff8...0x0fffffff => {
                        println!("End of Chain");
                        FatEntry::EndOfChain
                    },
                    n => FatEntry::Next(Cluster {
                        cluster_number: entry as u64,
                        parent_cluster: cluster.cluster_number
                    })
                }
            }
        };
        Ok(res)
    }


 }