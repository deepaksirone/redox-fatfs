use bpb::FATType;
use super::Result;
use std::io::{Read, Write, Seek, SeekFrom, ErrorKind, Error};
use std::cmp::min;

use filesystem::{FileSystem, Cluster, FsInfo};
use byteorder::{LittleEndian, ByteOrder, ReadBytesExt, WriteBytesExt};

pub const RESERVED_CLUSTERS: u64 = 2;

#[derive(Eq, PartialEq, Debug)]
pub enum FatEntry {
    Unused,
    Bad,
    EndOfChain,
    Next(Cluster)
}


#[inline]
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

pub fn get_entry<D: Read + Seek + Write>(fs: &mut FileSystem<D>, cluster: Cluster) -> Result<FatEntry> {
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
    //println!("[get_entry] FAT Offset: {:x} for cluster {:?}", get_fat_offset(fs.bpb.fat_type, cluster, fs.fat_start_sector(), fs.bytes_per_sec()), cluster);
    let fat_type = fs.bpb.fat_type;
    let fat_start_sector = fs.fat_start_sector();
    let bytes_per_sec = fs.bytes_per_sec();

    fs.seek_to(get_fat_offset(fat_type, cluster, fat_start_sector, bytes_per_sec))?;

    let res = match fs.bpb.fat_type {
        FATType::FAT12(_) => {
            let mut entry = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
            //let mut entry = LittleEndian::read_u16(&sectors[fat_ent_offset as usize ..(fat_ent_offset + 2) as usize]);
            entry = if current_cluster & 0x0001 > 0 { entry >> 4 }
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
            //println!("FAT32 entry for cluster {:?} = {:x}", cluster, entry);
            match entry {
                n if (cluster.cluster_number >= 0x0ffffff7 && cluster.cluster_number <= 0x0fffffff) => {
                    // Handling the case where the current cluster number is not an allocatable cluster number
                    // TODO: Should this panic or not
                    FatEntry::Bad
                },
                0 => FatEntry::Unused,
                0x0ffffff7 => FatEntry::Bad,
                0x0ffffff8...0x0fffffff => {
                    //println!("End of Chain");
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

pub fn get_entry_raw<D: Read + Seek + Write>(fs: &mut FileSystem<D>, cluster: Cluster) -> Result<u64> {
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
    //println!("[get_entry] FAT Offset: {:x} for cluster {:?}", get_fat_offset(fs.bpb.fat_type, cluster, fs.fat_start_sector(), fs.bytes_per_sec()), cluster);
    let fat_type = fs.bpb.fat_type;
    let fat_start_sector = fs.fat_start_sector();
    let bytes_per_sec = fs.bytes_per_sec();

    fs.seek_to(get_fat_offset(fat_type, cluster, fat_start_sector, bytes_per_sec))?;

    let res = match fs.bpb.fat_type {
        FATType::FAT12(_) => {
            let mut entry = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
            //let mut entry = LittleEndian::read_u16(&sectors[fat_ent_offset as usize ..(fat_ent_offset + 2) as usize]);
            entry = if current_cluster & 0x0001 > 0 { entry >> 4 }
                    else { entry & 0x0fff };
            entry as u64
        }
        FATType::FAT16(_) => {
            //let mut entry = LittleEndian::read_u16(&sectors[fat_ent_offset as usize ..(fat_ent_offset + 2) as usize]);
            let mut entry = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
            entry as u64
        }
        FATType::FAT32(_) => {
            //let mut entry = LittleEndian::read_u32(&sectors[fat_ent_offset as usize ..(fat_ent_offset + 4) as usize]) & 0x0fffffff;
            let mut entry = fs.disk.borrow_mut().read_u32::<LittleEndian>()? & 0x0FFFFFFF;
            //println!("FAT32 entry for cluster {:?} = {:x}", cluster, entry);
            entry as u64
        }
    };
    Ok(res)
}

pub fn get_free_cluster<D: Read + Write + Seek>(fs: &mut FileSystem<D>, start_cluster: Cluster,
                                                end_cluster: Cluster) -> Result<Cluster> {

    let max_cluster = fs.max_cluster_number();
    //println!("[get_free] Max Cluster = {:?}", max_cluster);
    let mut cluster = start_cluster.cluster_number;
    /*
    let fat_offset = match fs.bpb.fat_type {
        FATType::FAT12(_) => cluster + (cluster / 2),
        FATType::FAT16(_) => cluster * 2,
        FATType::FAT32(_) => cluster * 4,
    };

    let fat_sec_number = fat_start_sector + (fat_offset / bytes_per_sec);
    let fat_ent_offset = fat_offset % bytes_per_sec;*/
    //println!("[get_free] Fat Offset = {:X} for cluster = {:?}", get_fat_offset(fs.bpb.fat_type, start_cluster, fs.fat_start_sector(), fs.bytes_per_sec()), start_cluster.cluster_number);
    //fs.seek_to(get_fat_offset(fs.bpb.fat_type, start_cluster, fs.fat_start_sector(), fs.bytes_per_sec()))?;
    let fat_type = fs.bpb.fat_type;
    let fat_start_sector = fs.fat_start_sector();
    let bytes_per_sec = fs.bytes_per_sec();

    match fat_type {
        FATType::FAT12(_) => {
            fs.seek_to(get_fat_offset(fat_type, start_cluster, fat_start_sector, bytes_per_sec))?;
            let mut packed_val = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;


            loop {
                //println!("FAT12 Packed Val = {:X}", packed_val);

                let val = if cluster & 0x0001 > 0 { packed_val >> 4 } else { packed_val & 0x0fff };
                if val == 0 {
                    return Ok(Cluster::new(cluster as u64))
                }

                cluster += 1;
                if cluster == end_cluster.cluster_number || cluster == max_cluster.cluster_number {
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
            let fat_offset = get_fat_offset(fs.bpb.fat_type, start_cluster, fs.fat_start_sector(), fs.bytes_per_sec());
            fs.seek_to(fat_offset)?;
            while cluster < end_cluster.cluster_number && cluster < max_cluster.cluster_number {
                let mut packed_val = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
                if packed_val == 0 {
                    return Ok(Cluster::new(cluster as u64));
                }
                cluster += 1;
            }
            return Err(Error::new(ErrorKind::Other, "Space Exhausted on Disk"))
        },

        FATType::FAT32(_) => {
            /*let next_free = match fs.fs_info.borrow().get_next_free() {
                Some(x) => x,
                None => 0xFFFFFFFF
            };
            cluster = min(next_free, cluster);*/
            let fat_type = fs.bpb.fat_type;
            let fat_start_sector = fs.fat_start_sector();
            let bytes_per_sec = fs.bytes_per_sec();
            //println!("[get_free] Fat Offset = {:X} for cluster = {:?}", get_fat_offset(fs.bpb.fat_type, Cluster::new(cluster), fs.fat_start_sector(), fs.bytes_per_sec()), cluster);
            fs.seek_to(get_fat_offset(fat_type, Cluster::new(cluster), fat_start_sector, bytes_per_sec))?;
            while cluster < end_cluster.cluster_number && cluster < max_cluster.cluster_number {
                //let entry = get_entry(fs.bpb.fat_type, fs, Cluster::new(cluster)).ok();
                let val = fs.disk.borrow_mut().read_u32::<LittleEndian>()? & 0x0FFFFFFF;
                //println!("FAT32 entry for cluster {:?} = {:?}", cluster, entry);
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

pub fn set_entry<D: Read + Write + Seek>(fs: &mut FileSystem<D>, cluster: Cluster,
                                             fat_entry: FatEntry) -> Result<()> {
    let fat_offset = get_fat_offset(fs.bpb.fat_type, cluster, fs.fat_start_sector(), fs.bytes_per_sec());
    match fs.bpb.fat_type {
        FATType::FAT12(_) => {
            let raw_val = match fat_entry {
                FatEntry::Unused => 0,
                FatEntry::Bad => 0xff7,
                FatEntry::EndOfChain => 0xfff,
                FatEntry::Next(c) => c.cluster_number as u16
            };
            fs.seek_to(fat_offset)?;
            let old_val = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
            fs.seek_to(fat_offset)?;
            let new_val = if cluster.cluster_number & 0x0001 > 0 { (old_val & 0x000F) | (raw_val << 4) }
                                else { old_val & 0xF000 | raw_val };
            fs.disk.borrow_mut().write_u16::<LittleEndian>(new_val)?;
            Ok(())
        },
        FATType::FAT16(_) => {
            let raw_val = match fat_entry {
                FatEntry::Unused => 0,
                FatEntry::Bad => 0xfff7,
                FatEntry::EndOfChain => 0xffff,
                FatEntry::Next(c) => c.cluster_number as u16
            };
            fs.seek_to(fat_offset)?;
            fs.disk.borrow_mut().write_u16::<LittleEndian>(raw_val)?;
            Ok(())
        },
        FATType::FAT32(_) => {
            //fs.seek_to(fat_offset);
            let fat_size = fs.fat_size();
            let bound = if fs.mirroring_enabled() { 1 } else { fs.bpb.num_fats as u64 };
            for i in 0..bound {
                fs.seek_to(fat_offset + i * fat_size);
                let old_bits = fs.disk.borrow_mut().read_u32::<LittleEndian>()? & 0xF0000000;
                if fat_entry == FatEntry::Unused && cluster.cluster_number >= 0x0FFFFFF7 && cluster.cluster_number <= 0x0FFFFFFF {
                    warn!("Reserved Cluster {:?} cannot be marked as free", cluster);
                }

                let mut raw_val = match fat_entry {
                    FatEntry::Unused => 0,
                    FatEntry::Bad => 0x0FFFFFF7,
                    FatEntry::EndOfChain => 0x0FFFFFFF,
                    FatEntry::Next(c) => c.cluster_number as u32
                };
                raw_val = raw_val | old_bits;
                fs.seek_to(fat_offset + i as u64 * fat_size);
                fs.disk.borrow_mut().write_u32::<LittleEndian>(raw_val)?;
            }
            Ok(())
        }

    }
}


pub fn get_free_count<D: Read + Write + Seek>(fs: &mut FileSystem<D>, end_cluster: Cluster) -> Result<u64> {
    let mut count = 0;
    let mut cluster = RESERVED_CLUSTERS;
    match fs.bpb.fat_type {
        FATType::FAT12(_) => {
            let fat_offset = get_fat_offset(fs.bpb.fat_type, Cluster::new(cluster), fs.fat_start_sector(), fs.bytes_per_sec());
            fs.seek_to(fat_offset)?;
            let mut packed_val = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
            loop {
                let val = if cluster & 0x0001 > 0 { packed_val >> 4 } else { packed_val & 0x0fff };
                if val == 0 {
                    count += 1;
                }
                cluster += 1;
                if cluster == end_cluster.cluster_number {
                    fs.fs_info.borrow_mut().update_free_count(count);
                    return Ok(count)
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
            let fat_offset = get_fat_offset(fs.bpb.fat_type, Cluster::new(cluster), fs.fat_start_sector(), fs.bytes_per_sec());
            fs.seek_to(fat_offset)?;
            while cluster < end_cluster.cluster_number {
                let mut val = fs.disk.borrow_mut().read_u16::<LittleEndian>()?;
                if val == 0 {
                    count += 1;
                }
                cluster += 1;
            }
            fs.fs_info.borrow_mut().update_free_count(count);
            Ok(count)
        },
        FATType::FAT32(_) => {
            let fat_offset = get_fat_offset(fs.bpb.fat_type, Cluster::new(cluster), fs.fat_start_sector(), fs.bytes_per_sec());
            fs.seek_to(fat_offset)?;
            while cluster < end_cluster.cluster_number {
                let mut val = fs.disk.borrow_mut().read_u32::<LittleEndian>()? & 0x0FFFFFFF;
                if val == 0 {
                    count += 1;
                }
                cluster += 1;
            }
            fs.fs_info.borrow_mut().update_free_count(count);
            Ok(count)
        }
    }
}


pub fn allocate_cluster<D: Read + Write + Seek>(fs: &mut FileSystem<D>, prev_cluster: Option<Cluster>) -> Result<Cluster> {
    let end_cluster = fs.max_cluster_number();
    let mut start_cluster = match fs.bpb.fat_type {
        FATType::FAT32(_) => {
            let next_free = match fs.fs_info.borrow().get_next_free(end_cluster) {
                Some(x) => x,
                None => 0xFFFFFFFF
            };
            if next_free < end_cluster.cluster_number {
                Cluster::new(next_free)
            } else {
                Cluster::new(RESERVED_CLUSTERS)
            }
        },
        _ => Cluster::new(RESERVED_CLUSTERS),

    };

    let free_cluster = match get_free_cluster(fs, start_cluster, end_cluster) {
        Ok(c) => c,
        Err(_) if start_cluster.cluster_number > RESERVED_CLUSTERS => get_free_cluster(fs, Cluster::new(RESERVED_CLUSTERS), end_cluster)?,
        Err(e) => return Err(e)
    };

    set_entry(fs, free_cluster, FatEntry::EndOfChain)?;
    fs.fs_info.borrow_mut().delta_free_count(-1);
    fs.fs_info.borrow_mut().update_next_free(free_cluster.cluster_number + 1);
    if let Some(prev_clus) = prev_cluster {
        set_entry(fs, prev_clus, FatEntry::Next(free_cluster))?;
    }
    Ok(free_cluster)
}

pub fn deallocate_cluster<D: Read + Write + Seek>(fs: &mut FileSystem<D>, cluster: Cluster) -> Result<()> {
    let entry = get_entry(fs, cluster)?;
    if entry != FatEntry::Bad {
        set_entry(fs, cluster, FatEntry::Unused)?;
        fs.fs_info.borrow_mut().delta_free_count(1);
        Ok(())
    }
    else {
        Err(Error::new(ErrorKind::Other, "Bad clusters cannot be freed"))
    }

}

pub fn deallocate_cluster_chain<D: Read + Write + Seek>(fs: &mut FileSystem<D>, cluster: Cluster) -> Result<()> {
    let clusters = fs.clusters(cluster);
    for c in clusters {
        deallocate_cluster(fs, c)?;
    }
    Ok(())
}