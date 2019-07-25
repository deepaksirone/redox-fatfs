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
    let max_cluster = fs.max_cluster_number();
    println!("Root Cluster = {:?}", fs.clusters(root_clus));
    let mut buf = [0; 32];
    fs.read_cluster(Cluster::new(14), &mut buf);
    println!("Buffer = {:?}", buf);
    println!("BPB = {:?}", fs.bpb);
    println!("FsInfo = {:?}", fs.fs_info.borrow());
    println!("Mirroring Enabled = {:?}", fs.mirroring_enabled());

    //let free = get_free_cluster(&mut fs, Cluster::new(15), Cluster::new(100));
    //println!("Free Cluster = {:?}", free);
    let max_cluster = fs.max_cluster_number();
    println!("Free clusters from FsInfo = {:?}", fs.fs_info.borrow().get_free_count(max_cluster));
    println!("Num free Cluster = {:?}", get_free_count(&mut fs, max_cluster));
    println!("Cluster Chain of longFile.txt = {:?}", fs.clusters(Cluster::new(14)));
    println!("Clean shut bit = {:?}", fs.clean_shut_bit());
    println!("Hard Error bit = {:?}", fs.hard_error_bit());

    let dir_start = fs.root_dir_offset();
    println!("First Root Dir Entry: {:?} ", get_dir_entry_raw(&mut fs, dir_start).unwrap());
    println!("Second Root Dir Entry: {:?} ", get_dir_entry_raw(&mut fs, dir_start + 32).unwrap());
    println!("Third Root Dir Entry: {:?} ", get_dir_entry_raw(&mut fs, dir_start + 64).unwrap());
    let root_dir : Vec<DirEntry> = fs.root_dir().to_iter(&mut fs).collect();
    for entry in root_dir  {

        println!("Dir Entry : {:?}\n", entry);
        match entry {
            DirEntry::File(f) => {
                let tmp: Vec<char> = f.fname.chars().flat_map(|c| c.to_uppercase()).collect();
                println!("Upper case filename: {:?}", tmp)
            },
            DirEntry::Dir(d) => {
                let mut tmp: String = d.dir_name.chars().flat_map(|c| c.to_uppercase()).collect();
                tmp.retain(|c| (c != '\u{0}') && (c != '\u{FFFF}'));
                let m = tmp.chars().eq(d.dir_name.chars().flat_map(|c| c.to_uppercase()));
                println!("Upper case dirname: {:?}, match = {}", tmp, m)
            }
        }
    }
    let s = "//this/is/a/path.txt".to_string();
    let t : Vec<&str> = s.split('/').collect();
    println!("Split string : {:?}", t);


}


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
    println!("Cluster Chain of longFile.txt = {:?}", fs.clusters(Cluster::new(3)));
    let root_dir : Vec<DirEntry> = fs.root_dir().to_iter(&mut fs).collect();
    for entry in root_dir  {
        println!("Dir Entry : {:?}\n", entry);
    }

}


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
    println!("Cluster Chain of longFile.txt = {:?}", fs.clusters(Cluster::new(3)));
    let root_dir : Vec<DirEntry> = fs.root_dir().to_iter(&mut fs).collect();
    for entry in root_dir  {
        println!("Dir Entry : {:?}\n", entry);
    }
}

