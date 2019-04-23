use std::io::{Read, Write, Seek, SeekFrom};
use std::path::Path;
use super::Result;
use BiosParameterBlock;


pub struct FileSystem<D: Read + Write + Seek> {
    pub disk: D,
    pub bpb: BiosParameterBlock,
}

impl<D: Read + Write + Seek> FileSystem<D> {

    pub fn from_offset(partition_offset: u64, mut disk: D) -> Result<FileSystem<D>> {
        disk.seek(SeekFrom::Start(partition_offset))?;
        let bpb = BiosParameterBlock::populate(&mut disk)?;

        Ok(FileSystem {
            disk: disk,
            bpb: bpb
        })
    }

    


}
