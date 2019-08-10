
use std::io::{Read, Seek, SeekFrom};
use std::default::Default;
use std::fmt;
use super::Result;

use byteorder::{ReadBytesExt, LittleEndian};
//use Disk;


/// The BIOS Parameter Block elements common to all types of FAT volumes
#[allow(dead_code)]
#[derive(Clone, Copy, Default, Debug)]
pub struct BiosParameterBlock {
    /// Jump instructions to boot code
    /// BS_jmpBoot
    pub jmp_boot: [u8; 3],
    /// Indicates the OS which formatted this volume
    /// BS_OEMName
    pub oem_name: [u8; 8],
    /// Size of a sector
    /// BPB_BytsPerSec
    pub bytes_per_sector: u16,
    /// Sectors per cluster
    /// BPB_SecPerClus
    pub sectors_per_cluster: u8,
    /// Reserved sectors count
    /// BPB_RsvdSecCnt
    pub rsvd_sec_cnt: u16,
    /// Count of FAT data structures in the volume
    /// BPB_NumFATs
    pub num_fats: u8,
    /// Count of 32-byte dir entries in root dir(Only for FAT12 and FAT16)
    /// BPB_RootEntCnt
    pub root_entries_cnt: u16,
    /// 16 bit total count of sectors on volume(Only for FAT12 and FAT16)
    /// BPB_TotSec16
    pub total_sectors_16: u16,
    /// Type of media
    /// BPB_Media
    pub media: u8,
    /// Count of sectors used by one FAT(Only for FAT12 and FAT16)
    /// BPB_FATSz16
    pub fat_size_16: u16,
    /// Sectors per track
    /// BPB_SecPerTrk
    pub sectors_per_track: u16,
    /// Number of heads
    /// BPB_NumHeads
    pub number_of_heads: u16,
    /// Count of Hidden Sectors preceding this partition
    /// BPB_HiddSec
    pub hidden_sectors: u32,
    /// Total Sectors
    /// BPB_TotSec32
    pub total_sectors_32: u32,
    /// Enum wrapping FAT specific struct
    pub fat_type: FATType,
    /// BootSignature
    pub sig: [u8; 2]
}

#[derive(Copy, Clone, Debug)]
pub enum FATType {
    FAT32(BiosParameterBlockFAT32),
    FAT12(BiosParameterBlockLegacy),
    FAT16(BiosParameterBlockLegacy)
}


/// Bios Parameter Block for FAT12 and FAT16 volumes
#[derive(Copy, Clone, Default)]
pub struct BiosParameterBlockLegacy {
    /// Drive number for BIOS INT 0x13
    /// BS_DrvNum
    pub drive_num: u8,
    /// Reserved
    /// BS_Reserverd1
    pub reserved: u8,
    /// Extended boot signature (0x29)
    /// BS_BootSig
    pub boot_sig: u8,
    /// Volume serial number
    /// BS_VolID
    pub vol_id: u32,
    /// Volume Label
    /// BS_VolLab
    pub volume_label: [u8; 11],
    /// File System Type
    /// BS_FilSysType
    pub file_sys_type: u32,
    //// Boot Code
    //pub code : [u8; 452]
}


#[derive(Copy, Clone, Default)]
pub struct BiosParameterBlockFAT32 {
    /// FAT32 Count of sectors occupied by one FAT
    /// BPB_FATSz32
    pub fat_size: u32,
    /// Extended Flags
    /// BPB_ExtFlags
    /// Bits 0-3 -- Zero based number of active FAT
    /// Only valid if mirroring iFAT32s disabled
    /// Bits 4-6 -- Reserved
    /// Bit 7 -- 0 means the FAT is mirrored at runtime into all FATs
    ///     -- 1 means only one FAT is active and is referenced by bits 0-3
    /// Bits 8-15 -- reserved
    pub ext_flags: u16,
    /// FS Version: High byte is the major revision number
    /// Low byte is the minor revision number
    /// BPB_FSVer
    pub fs_ver: u16,
    /// Cluster number of the first cluster of the root directory
    /// BPB_RootClus
    pub root_cluster: u32,
    /// Sector number of the FSINFO structure in the reserved area of the FAT32 volume
    /// BPB_FSInfo
    pub fs_info: u16,
    /// If non zero indicates the sector number in the reserved area of the volume of a copy of the
    /// boot record
    /// BPB_BkBootSec
    pub bk_boot_sec: u16,
    /// Reserved field
    /// BPB_Reserved
    pub reserved: [u8; 12],
    /// Drive number
    /// BS_DrvNum
    pub drv_num: u8,
    /// Reserved1
    /// BS_Reserved1
    pub reserved1: u8,
    /// Boot Signature
    /// BS_BootSig
    pub boot_sig: u8,
    /// Volume ID
    /// BS_VolID
    pub vol_id: u32,
    /// Volume label
    /// BS_VOlLab
    pub volume_label: [u8; 11],
    /// File System type
    /// BS_FilSystype
    pub file_sys_type: [u8; 8],
    //// Boot Code
    //pub code: [u8; 420]
}

impl BiosParameterBlock {
    pub fn populate<D: Read+Seek>(disk: &mut D) -> Result<BiosParameterBlock> {

        let mut bpb  = BiosParameterBlock::default();
        println!("Over Here!");
        disk.read(&mut bpb.jmp_boot)?;
        disk.read(&mut bpb.oem_name)?;
        println!("Over Here! 1");
        bpb.bytes_per_sector = disk.read_u16::<LittleEndian>()?;
        bpb.sectors_per_cluster = disk.read_u8()?;
        bpb.rsvd_sec_cnt = disk.read_u16::<LittleEndian>()?;
        bpb.num_fats = disk.read_u8()?;
        bpb.root_entries_cnt = disk.read_u16::<LittleEndian>()?;
        bpb.total_sectors_16 = disk.read_u16::<LittleEndian>()?;
        bpb.media = disk.read_u8()?;
        bpb.fat_size_16 = disk.read_u16::<LittleEndian>()?;
        bpb.sectors_per_track = disk.read_u16::<LittleEndian>()?;
        bpb.number_of_heads = disk.read_u16::<LittleEndian>()?;
        bpb.hidden_sectors = disk.read_u32::<LittleEndian>()?;
        bpb.total_sectors_32 = disk.read_u32::<LittleEndian>()?;

        let mut bpb32 = BiosParameterBlockFAT32::default();
        bpb32.fat_size = disk.read_u32::<LittleEndian>()?;
        bpb32.ext_flags = disk.read_u16::<LittleEndian>()?;
        bpb32.fs_ver = disk.read_u16::<LittleEndian>()?;
        bpb32.root_cluster = disk.read_u32::<LittleEndian>()?;
        bpb32.fs_info = disk.read_u16::<LittleEndian>()?;
        bpb32.bk_boot_sec = disk.read_u16::<LittleEndian>()?;
        disk.read(&mut bpb32.reserved)?;
        bpb32.drv_num = disk.read_u8()?;
        bpb32.reserved1 = disk.read_u8()?;
        bpb32.boot_sig = disk.read_u8()?;
        bpb32.vol_id = disk.read_u32::<LittleEndian>()?;
        disk.read(&mut bpb32.volume_label)?;
        disk.read(&mut bpb32.file_sys_type)?;
        //disk.read_exact(&mut bpb32.code)?;
        disk.seek(SeekFrom::Current(420))?;
        disk.read(&mut bpb.sig)?;

        let root_sectors = ((bpb.root_entries_cnt as u32 * 32) + (bpb.bytes_per_sector as u32) - 1) / (bpb.bytes_per_sector as u32);
        let fat_sz = if bpb.fat_size_16 != 0 { bpb.fat_size_16 as u32 } else { bpb32.fat_size };
        let tot_sec = if bpb.total_sectors_16 != 0 { bpb.total_sectors_16 as u32 } else { bpb.total_sectors_32 };
        let data_sec = tot_sec - ((bpb.rsvd_sec_cnt as u32) + (bpb.num_fats as u32) * fat_sz + root_sectors);

        let count_clusters = data_sec / (bpb.sectors_per_cluster as u32);
        bpb.fat_type = if count_clusters < 4085 { FATType::FAT12(BiosParameterBlockLegacy::default()) }
                       else if count_clusters < 65525 { FATType::FAT16(BiosParameterBlockLegacy::default()) }
                       else { FATType::FAT32(bpb32) };

        Ok(bpb)
    }

    pub fn validate(&self) -> bool {
        //TODO: Add validity checks
        true
    }

}

#[allow(dead_code)]
impl Default for FATType {
    fn default() -> Self {
        let f = BiosParameterBlockFAT32::default();
        FATType::FAT32(f)
    }
}

#[allow(dead_code)]
impl fmt::Debug for BiosParameterBlockFAT32 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "BiosParameterBlockFAT32 {{
                fat_size: {:?},
                ext_flags: {:?},
                fs_ver: {:?},
                root_cluster: {:?},
                fs_info: {:?},
                bk_boot_sec: {:?},
                drv_num: {:?},
                boot_sig: {:?},
                vol_id: {:?}
                 }}", self.fat_size, self.ext_flags,self.fs_ver, self.root_cluster, self.fs_info,
                self.bk_boot_sec, self.drv_num, self.boot_sig, self.vol_id)
    }
}

#[allow(dead_code)]
impl fmt::Debug for BiosParameterBlockLegacy {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "BiosParameterBlockLegacy {{
            drive_num: {},
            reserved: {},
            boot_sig: {},
            vol_id: {},
            file_sys_type: {}
        }}", self.drive_num, self.reserved, self.boot_sig, self.vol_id, self.file_sys_type)
    }
}
