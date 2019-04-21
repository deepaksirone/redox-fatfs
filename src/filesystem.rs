use std::io::{Read, Write, Seek};
use BiosParameterBlock;

pub struct FileSystem<D: Read + Write + Seek> {
    pub disk: D,
    pub bpb: BiosParameterBlock,
}

impl<D: Read + Write + Seek> FileSystem<D> {

    pub fn init(mut disk: D) -> FileSystem<D> {
        unimplemented!();
    }
}
