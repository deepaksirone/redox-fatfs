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

