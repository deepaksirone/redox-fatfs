pub struct File {
    pub first_cluster : Option<u32>,
    pub current_cluster : Option<u32>,
    pub filepath : Option<String>,
    pub offset : u32,
    // FIXME: Add pointer to dirctory entry
}
