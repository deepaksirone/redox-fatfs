#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FmapKey {
    pub block: u64,
    pub offset: usize,
    pub size: usize
}

#[derive(Clone)]
pub struct FmapValue {
    pub buffer: Vec<u8>,
    /// The actual file length. Syncing only writes &buffer[..actual_size].
    pub actual_size: usize,
    pub refcount: usize
}

pub struct Fmaps(Vec<Option<(FmapKey, FmapValue)>>);