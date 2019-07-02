extern crate redox_fatfs;

use std::fs;
use std::io::prelude::*;
use std::io::SeekFrom;
use std::str;

use redox_fatfs::*;

#[test]
fn print_bpb() {
    let mut f = fs::File::open("images/fat32.img").unwrap();
    let mut fs = redox_fatfs::FileSystem::from_offset(0, f).expect("Parsing Error");
    let root_clus = Cluster::new(10);
    println!("Root Cluster = {:?}", fs.clusters(root_clus));
    let mut buf = [0; 32];
    fs.read_cluster(root_clus, &mut buf);
    println!("Buffer = {:?}", buf);
    println!("BPB = {:?}", fs.bpb);
    println!("FsInfo = {:?}", fs.fs_info.borrow());
    println!("Mirroring Enabled = {:?}", fs.mirroring_enabled());

    let free = get_free_cluster(&mut fs, Cluster::new(11), Cluster::new(100));
    println!("Free Cluster = {:?}", free);


}

