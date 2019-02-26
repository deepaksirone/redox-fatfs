
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
    pub number_of_heads: u16
    /// Count of Hidden Sectors preceding this partition
    /// BPB_HiddSec
    pub hidden_sectors: u32,
    /// Total Sectors
    /// BPB_TotSec32
    pub total_sectors_32: u32

}

pub struct File {
    pos: u64,
}

