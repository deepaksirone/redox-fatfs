#![allow(dead_code)]
#![allow(unused_must_use)]
extern crate redox_fatfs;

use std::fs::OpenOptions;
use std::str;
use std::io::{Seek, SeekFrom, Read, Cursor};

use redox_fatfs::*;


fn print_fat32() {
    let f = OpenOptions::new().read(true).write(true).open("images/fat32.img").expect("Failed to open file");
    let mut fs = redox_fatfs::FileSystem::from_offset(0, f).expect("Parsing Error");
    let root_clus = Cluster::new(2);
    let _max_cluster = fs.max_cluster_number();
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
    let mut root_dir : Vec<DirEntry> = fs.root_dir().to_iter(&mut fs).collect();
    let mut file_buf = [0; 3000];
    for entry in root_dir.iter_mut()  {

        println!("Dir Entry : {:?}\n", entry);
        println!("Short Name: {:?}\n", entry.short_name());
        match entry {
            DirEntry::File(ref mut f) => {
                let tmp: Vec<char> = f.fname.chars().flat_map(|c| c.to_uppercase()).collect();
                let len = f.read(&mut file_buf, &mut fs, 0).expect("Error Reading file");
                println!("Upper case filename: {:?}", tmp);
                for c in &file_buf[..len] {
                    print!("{}", *c as char);
                }
                println!("Read len = {}", len);
                let w = f.write(&[0x45,
                0x78,
                0x74,
                0x72,
                0x61,
                0x20,
                0x74,
                0x65,
                0x78,
                0x74
                ], &mut fs, f.size() + 25).expect("Write failed");
                println!("Written bytes = {:?}", w);
            },
            DirEntry::Dir(d) => {
                let mut tmp: String = d.dir_name.chars().flat_map(|c| c.to_uppercase()).collect();
                tmp.retain(|c| (c != '\u{0}') && (c != '\u{FFFF}'));
                let m = tmp.chars().eq(d.dir_name.chars().flat_map(|c| c.to_uppercase()));
                println!("Upper case dirname: {:?}, match = {}", tmp, m)
            },
            DirEntry::VolID(s) => {
                println!("[VOL-ID] The volume ID: {:?}", s);
            }
        }
    }
    let s = "//this/is/a/path.txt".to_string();
    let t : Vec<&str> = s.split('/').collect();
    println!("Split string : {:?}", t);
    let root_d = fs.root_dir();
    let r = root_d.find_entry("heLlo.txt", None, None, &mut fs);
    println!("Trying to find heLlo.txt : {:?}", r);

    println!("Attempting to remove hello.txt: {:?}", root_d.remove("/hello.txt", &mut fs));
    println!("Attempting to remove someDir: {:?}", root_d.remove("/someDir", &mut fs));
    let hello = root_d.create_file("/hello5.txt", &mut fs).expect("Error Creating hello.txt");
    println!("Created hello1.txt");
    let r = Dir::rename(&mut DirEntry::File(hello), "/hello2.txt", &mut fs);
    println!("Attempting to move hello1.txt to hello2.txt : {:?}", r);
    let mut dir1 = root_d.create_dir("/dir5/", &mut fs).expect("Error creating dir1");
    println!("Created dir5");
    let mut dir2 = dir1.create_dir("/dir6", &mut fs).expect("Error creating dir2");
    println!("Created dir6");
    let mut hello2 = dir2.create_file("/hello2.txt/", &mut fs).expect("Error creating hello2.txt");
    hello2.write("This is something in hello2".as_bytes(), &mut fs, 0).expect("Failed to write hello2.txt");


}

fn print_fat12() {
    let f = OpenOptions::new().read(true).write(true).open("images/fat12.img").expect("Failed to open fat12.img");
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
    let mut root_dir : Vec<DirEntry> = fs.root_dir().to_iter(&mut fs).collect();
    let mut file_buf = [0; 3000];
    for entry in root_dir.iter_mut()  {

        println!("Dir Entry : {:?}\n", entry);
        println!("Short Name: {:?}\n", entry.short_name());
        match entry {
            DirEntry::File(ref mut f) => {
                let tmp: Vec<char> = f.fname.chars().flat_map(|c| c.to_uppercase()).collect();
                let len = f.read(&mut file_buf, &mut fs, 0).expect("Error Reading file");
                println!("Upper case filename: {:?}", tmp);
                for c in &file_buf[..len] {
                    print!("{}", *c as char);
                }
                println!("Read len = {}", len);
                let w = f.write(&[0x45,
                    0x78,
                    0x74,
                    0x72,
                    0x61,
                    0x20,
                    0x74,
                    0x65,
                    0x78,
                    0x74
                ], &mut fs, f.size() + 25).expect("Write failed");
                println!("Written bytes = {:?}", w);
            },
            DirEntry::Dir(d) => {
                let mut tmp: String = d.dir_name.chars().flat_map(|c| c.to_uppercase()).collect();
                tmp.retain(|c| (c != '\u{0}') && (c != '\u{FFFF}'));
                let m = tmp.chars().eq(d.dir_name.chars().flat_map(|c| c.to_uppercase()));
                println!("Upper case dirname: {:?}, match = {}", tmp, m)
            },
            DirEntry::VolID(s) => {
                println!("[VOL-ID] The volume ID: {:?}", s);
            }
        }
    }

}


fn print_fat16() {
    let f = OpenOptions::new().read(true).write(true).open("images/fat16.img").expect("Failed to open fat16.img");
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


fn short_names()
{
    let s = ".";
    let s1 = "....hello...txt";
    let p = "/this/is/a/path/a.txt/";
    let e = s.rfind('.').unwrap();
    println!(". r find: {:?}", e);
    println!("Printing slice: {:?}", &s.as_bytes()[..e]);
    println!("IStrue : {:?}", s == ".");
    println!("Period Stripped: {:?}", s1.trim_start_matches("."));
    println!("Reverse Split path :{:?}", rsplit_path(p));
    let name = ".bashrc.swp";
    let mut sng = ShortNameGen::new(name);
    let i1 = sng.generate().expect("Failed to generate");
    println!(".bashrc.swp 1 iter : {:?}", str::from_utf8(&i1));
    sng.add_name(&i1);

    let i2 = sng.generate().expect("Failed to generate");
    println!(".bashrc.swp 2 iter : {:?}", str::from_utf8(&i2));
    sng.add_name(&i2);

    let i3 = sng.generate().expect("Failed to generate");
    println!(".bashrc.swp 3 iter : {:?}", str::from_utf8(&i3));
    sng.add_name(&i3);

    let i4 = sng.generate().expect("Failed to generate");
    println!(".bashrc.swp 4 iter : {:?}", str::from_utf8(&i4));
    sng.add_name(&i4);

    let i5 = sng.generate().expect("Failed to generate");
    println!(".bashrc.swp 5 iter : {:?}", str::from_utf8(&i5));
    sng.add_name(&i5);

    let i6 = sng.generate().expect("Failed to generate");
    println!(".bashrc.swp 3 iter : {:?}", str::from_utf8(&i6));
    sng.add_name(&i6);

    let i7 = sng.generate().expect("Failed to generate");
    println!(".bashrc.swp 3 iter : {:?}", str::from_utf8(&i7));
    sng.add_name(&i7);

    let _i8 = sng.generate().expect("Failed to generate");
    println!(".bashrc.swp 3 iter : {:?}", str::from_utf8(&_i8));
    sng.add_name(&_i8);

    let i9 = sng.generate().expect("Failed to generate");
    println!(".bashrc.swp 3 iter : {:?}", str::from_utf8(&i9));
    sng.add_name(&i9);

    sng.next_iteration();
    let i10 = sng.generate().expect("Failed to generate");
    println!(".bashrc.swp 3 iter : {:?}", str::from_utf8(&i10));
    sng.add_name(&i10);

}

fn rsplit_path(path: &str) -> (&str, Option<&str>) {
    let mut path_split = path.trim_matches('/').rsplitn(2, "/");
    let comp = path_split.next().unwrap();
    let rest_opt = path_split.next();
    (comp, rest_opt)
}

fn split_path(path: &str) -> (&str, Option<&str>) {
    let mut path_split = path.trim_matches('/').splitn(2, "/");
    let comp = path_split.next().unwrap();
    let rest_opt = path_split.next();
    (comp, rest_opt)
}

fn test_cursor() {
    let mut v = vec![1, 2, 3, 4, 5, 6];
    let mut cursor = Cursor::new(v);
    cursor.seek(SeekFrom::Start(2));
    for x in cursor.get_ref() {
        println!("Val = {}", x);
    }
}

#[test]
fn test_split() {
    let path = "";
    println!("Split path: {:?}", split_path(path));
}