use bpb::FATType;
use super::Result;
use std::io::{Read, Write, Seek, SeekFrom, ErrorKind, Error};
use std::cmp::min;

use filesystem::{FileSystem, Cluster, FsInfo};
use byteorder::{LittleEndian, ByteOrder, ReadBytesExt};

#[derive(Eq, PartialEq)]
pub enum FatEntry {
    Unused,
    Bad,
    EndOfChain,
    Next(Cluster)
}

fn get_fat_offset(fat_type: FATType, cluster: Cluster, fat_start_sector: u64, bytes_per_sec: u64) -> u64 {
    let current_cluster = cluster.cluster_number;
    let fat_offset = match fat_type {
        FATType::FAT12(_) => current_cluster + (current_cluster / 2),
        FATType::FAT16(_) => current_cluster * 2,
        FATType::FAT32(_) => current_cluster * 4,
    };

    let fat_sec_number = fat_start_sector + (fat_offset / bytes_per_sec);
    let fat_ent_offset = fat_offset % bytes_per_sec;

    fat_sec_number * bytes_per_sec + fat_ent_offset
}

pub fn get_entry<D: Read + Seek + Write>(fat_type: FATType, fs: &mut FileSystem<D>, cluster: Cluster) -> Result<FatEntry> {
    let current_cluster = cluster.cluster_number;
    /*
    let fat_offset = match fat_type {
            FATType::FAT12(_) => current_cluster + (current_cluster / 2),
            FATType::FAT16(_) => current_cluster * 2,
            FATType::FAT32(_) => current_cluster * 4,
    };

    let fat_start_sector = fs.fat_start_sector();
    let bytes_per_sec = fs.bytes_per_sec();

    let fat_sec_number = fat_start_sector + (fat_offset / bytes_per_sec);
    let fat_ent_offset = fat_offset % bytes_per_sec;
    //let mut sectors: [u8; 8192] = [0; 2 * 4096];
    //fs.read_at(fat_sec_number * bytes_per_sec, &mut sectors[..((bytes_per_sec * 2) as usize)]);*/

    fs.seek_to(get_fat_offset(fat_type, cluster, fs.fat_start_sector(), fs.bytes_per_sec()))?;

    let res = match fat_type {
        FATType::FAT12(_) => {
            let mut entry = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
            //let mut entry = LittleEndian::read_u16(&sectors[fat_ent_offset as usize ..(fat_ent_offset + 2) as usize]);
            entry = if current_cluster & 0x0001 == 1 { entry >> 4 }
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
            let mut entry = fs.disk.borrow_mut().read_u32::<LittleEndian>()? & 0x0FFFFFFF;
            println!("FAT32 entry for cluster {:?} = {:x}", cluster, entry);
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

pub fn get_free_cluster<D: Read + Write + Seek>(fs: &mut FileSystem<D>, start_cluster: Cluster,
                                                end_cluster: Cluster) -> Result<Cluster> {

    let max_cluster = fs.max_cluster_number();

    let mut cluster = start_cluster.cluster_number;
    /*
    let fat_offset = match fs.bpb.fat_type {
        FATType::FAT12(_) => cluster + (cluster / 2),
        FATType::FAT16(_) => cluster * 2,
        FATType::FAT32(_) => cluster * 4,
    };

    let fat_sec_number = fat_start_sector + (fat_offset / bytes_per_sec);
    let fat_ent_offset = fat_offset % bytes_per_sec;*/

    fs.seek_to(get_fat_offset(fs.bpb.fat_type, start_cluster, fs.fat_start_sector(), fs.bytes_per_sec()))?;

    match fs.bpb.fat_type {
        FATType::FAT12(_) => {
            let mut packed_val = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
            loop {

                let val = if cluster & 0x1 == 1 { packed_val >> 4 } else { packed_val & 0x0fff };
                if val == 0 {
                    return Ok(Cluster::new(val as u64))
                }

                cluster += 1;
                if cluster == end_cluster.cluster_number {
                    return Err(Error::new(ErrorKind::Other, "Space Exhausted on Disk"))
                }

                packed_val = match cluster & 1 {
                    0 => fs.disk.borrow_mut().read_u16::<LittleEndian>()?,
                    _ => {
                        let next_byte = fs.disk.borrow_mut().read_u8()? as u16;
                        (packed_val >> 8) | (next_byte << 8)
                    },
                };

            }
        },

        FATType::FAT16(_) => {
            while cluster < end_cluster.cluster_number {
                let mut packed_val = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
                if packed_val == 0 {
                    return Ok(Cluster::new(packed_val as u64));
                }
                cluster += 1;
            }
            return Err(Error::new(ErrorKind::Other, "Space Exhausted on Disk"))
        },

        FATType::FAT32(_) => {
            cluster = min(fs.fs_info.borrow().get_next_free(), cluster);
            while cluster < end_cluster.cluster_number && cluster < max_cluster.cluster_number {
                //let entry = get_entry(fs.bpb.fat_type, fs, Cluster::new(cluster)).ok();
                let val = fs.disk.borrow_mut().read_u32::<LittleEndian>()? & 0x0FFFFFFF;
                /*if entry == Some(FatEntry::Unused) {
                    return Ok(Cluster::new(cluster))
                }*/
                if val == 0 {
                    return Ok(Cluster::new(cluster))
                }
                cluster += 1;
            }
            return Err(Error::new(ErrorKind::Other, "Space Exhausted on Disk"))
        }
    }
}

pub fn set_entry<D: Read + Write + Seek>(fat_type: FATType, fs: &mut FileSystem<D>, cluster: Cluster,
                                             next_cluster: Cluster) -> Result<()> {
    unimplemented!()
}

pub fn get_free_count<D: Read + Write + Seek>(fat_type: FATType, fs: &mut FileSystem<D>) -> Result<()> {
    unimplemented!()
}