use std::ops::{Deref, DerefMut};
use std::io::{Read, Write, Seek};
use std::default::Default;

use Disk;
use BLOCK_SIZE;

/// The BIOS Parameter Block elements common to all types of FAT volumes
#[derive(Default, Clone, Copy)]
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

#[derive(Copy, Clone)]
pub enum FATType {
    FAT32(BiosParameterBlockFAT32),
    FATLegacy(BiosParameterBlockLegacy)
}

/// Bios Parameter Block for FAT12 and FAT16 volumes
#[derive(Copy, Clone)]
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
    /// Boot Code
    pub code : [u8; 452]
}


#[derive(Copy, Clone)]
pub struct BiosParameterBlockFAT32 {
    /// FAT32 Count of sectors occupied by one FAT
    /// BPB_FATSz32
    pub fat_size: u32,
    /// Extended Flags
    /// BPB_ExtFlags
    /// Bits 0-3 -- Zero based number of active FAT
    /// Only valid if mirroring is disabled
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
    /// Boot Code
    pub code: [u8; 420]
}

impl BiosParameterBlock {
    pub fn populate<D: Read>(disk: &mut D) -> BiosParameterBlock {
        let mut bpb : BiosParameterBlock = Default::default();
        disk.read_exact(&mut bpb.jmp_boot);
        disk.read_exact(&mut bpb.oem_name);
        bpb
    }

}

impl Default for BiosParameterBlockLegacy {
    fn default() -> Self {
         BiosParameterBlockLegacy {
             code: [0; 452],
             ..Default::default()

         }
    }
}

impl Default for BiosParameterBlockFAT32 {
    fn default() -> Self {
        BiosParameterBlockFAT32 {
            code: [0; 420],
            ..Default::default()
        }
    }
}

impl Default for FATType {
    fn default() -> Self {
        let f: BiosParameterBlockFAT32 = Default::default();
        FATType::FAT32(f)
    }
}
