
bitflags! {

    #[derive(Default)]
    pub struct FileAttributes: u8 {
        const RD_ONLY   = 0x01;
        const HIDDEN    = 0x02;
        const SYSTEM    = 0x04;
        const VOLUME_ID = 0x08;
        const DIRECTORY = 0x10;
        const ARCHIVE   = 0x20;
        const LFN       = Self::RD_ONLY.bits | Self::HIDDEN.bits
                            | Self::SYSTEM.bits | Self::VOLUME_ID.bits;
   }
}

pub struct File {
    pub first_cluster : u64,
    pub current_cluster : u64,
    pub filepath : String,
    pub offset : u64,
    // FIXME: Add pointer to directory entry
}

pub struct Dir {
    pub first_cluster: u64,
    pub current_cluster: u64,
    pub dirpath : String,
    pub offset: u64,
}

#[derive(Debug, Default, Copy, Clone)]
pub struct ShortDirEntry {
    /// Short name
    dir_name: [u8; 11],
    /// File Attributes
    file_attrs: FileAttributes,
    /// Win NT reserved
    nt_res: u8,
    /// Millisecond part of file creation time
    crt_time_tenth: u8,
    /// Time of file creation
    crt_time: u16,
    /// Date of file cretion
    crt_date: u16,
    /// Last access date
    lst_acc_date: u16,
    /// High word of first cluster(0 for FAT12 and FAT16)
    fst_clst_hi: u16,
    /// Last write time
    wrt_time: u16,
    /// Last write date
    wrt_date: u16,
    /// Low word of first cluster
    fst_clus_lo: u16,
    /// File Size
    file_size: u32
}

pub struct LongDirEntry {
    /// Ordinal of the entry
    ord: u8,
    /// Characters 1-5 of name
    name1: [u8; 10],
    /// File Attributes
    file_attrs: FileAttributes,
    /// Entry Type: If zero indicates that the entry
    /// is a subcomponent of a long name
    /// Non-zero values are reserved
    dirent_type: u8,
    /// Checksum computed from short name
    chksum: u8,
    /// Characters 6-11 of name
    name2: [u8; 10],
    /// FirstCluster Low Word
    /// Should be zero in a long file entry
    first_clus_low: u16,
    /// Characters 12-13 of name
    name3: [u8; 4]
}

pub enum DirEntry {

}