extern crate redox_fatfs;

use std::fs;
use std::io::prelude::*;
use std::io::SeekFrom;
use std::str;

use redox_fatfs::*;

#[test]
fn print_fat32() {
    let mut f = fs::File::open("images/fat32.img").unwrap();
    let mut fs = redox_fatfs::FileSystem::from_offset(0, f).expect("Parsing Error");
    let root_clus = Cluster::new(2);
    println!("Root Cluster = {:?}", fs.clusters(root_clus));
    let mut buf = [0; 32];
    fs.read_cluster(root_clus, &mut buf);
    println!("Buffer = {:?}", buf);
    println!("BPB = {:?}", fs.bpb);
    println!("FsInfo = {:?}", fs.fs_info.borrow());
    println!("Mirroring Enabled = {:?}", fs.mirroring_enabled());

    let free = get_free_cluster(&mut fs, Cluster::new(15), Cluster::new(100));
    println!("Free Cluster = {:?}", free);
    let max_cluster = fs.max_cluster_number();
    println!("Num free Cluster = {:?}", get_free_count(&mut fs, max_cluster));

}

#[test]
fn print_fat12() {
    let mut f = fs::File::open("images/fat12.img").unwrap();
    let mut fs = redox_fatfs::FileSystem::from_offset(0, f).expect("Parsing Error");
    let root_sec = fs.bpb.rsvd_sec_cnt as u64 + (fs.bpb.num_fats as u64 * fs.bpb.fat_size_16 as u64);
    let root_clus = Cluster::new(root_sec / fs.bpb.sectors_per_cluster as u64);
    println!("Root Cluster = {:?}", fs.clusters(root_clus));
    println!("First Data Sec = {}", fs.first_data_sec);
    // Cluster 2 starts from first_data_sec sector onwards

    let mut buf = [0; 32];
    fs.read_sector(root_sec, &mut buf);
    println!("Buffer = {:?}", buf);
    println!("BPB = {:?}", fs.bpb);
    println!("FsInfo = {:?}", fs.fs_info.borrow());
    println!("Mirroring Enabled = {:?}", fs.mirroring_enabled());

    fs.read_cluster(Cluster::new(7), &mut buf);
    println!("somefile.txt = {:?}", buf);

    let free = get_free_cluster(&mut fs, Cluster::new(7), Cluster::new(100));
    println!("Free Cluster = {:?}", free);
    let max_cluster = fs.max_cluster_number();
    println!("Num free Cluster = {:?}", get_free_count(&mut fs, max_cluster));

}

#[test]
fn print_fat16() {
    let mut f = fs::File::open("images/fat16.img").unwrap();
    let mut fs = redox_fatfs::FileSystem::from_offset(0, f).expect("Parsing Error");
    let max_cluster = fs.max_cluster_number();
    let root_sec = fs.bpb.rsvd_sec_cnt as u64 + (fs.bpb.num_fats as u64 * fs.bpb.fat_size_16 as u64);
    let root_clus = Cluster::new(root_sec / fs.bpb.sectors_per_cluster as u64);
    println!("Root Cluster = {:?}", fs.clusters(root_clus));
    println!("First Data Sec = {}", fs.first_data_sec);
    // Cluster 2 starts from first_data_sec sector onwards

    let mut buf = [0; 32];
    fs.read_sector(root_sec, &mut buf);
    println!("Buffer = {:?}", buf);
    println!("BPB = {:?}", fs.bpb);
    println!("FsInfo = {:?}", fs.fs_info.borrow());
    println!("Mirroring Enabled = {:?}", fs.mirroring_enabled());

    fs.read_cluster(Cluster::new(5), &mut buf);
    println!("somefile.txt = {:?}", buf);

    let free = get_free_cluster(&mut fs, Cluster::new(5), Cluster::new(100));
    println!("Free Cluster = {:?}", free);
    println!("Num free Cluster = {:?}", get_free_count(&mut fs, max_cluster));
}

