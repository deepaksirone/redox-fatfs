struct BiosParameterBlock {
    // Jump instructions to boot code
    boot_jmp: [u8; 3],
    oem_name: [u8; 8],
    // Size of a sector, actual size: 2 bytes
    sector_size: u32,
    
}
struct File {
    pos: u64,
}
