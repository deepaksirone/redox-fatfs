#![deny(warnings)]
#![cfg_attr(unix, feature(libc))]

#[cfg(unix)]
extern crate libc;

extern crate redox_fatfs;

#[cfg(target_os = "redox")]
extern crate syscall;

extern crate uuid;

use std::env;
use std::fs::File;
use std::io::{Read, Write};
use std::os::unix::io::FromRawFd;
use std::process;
use std::sync::atomic::{AtomicUsize, Ordering};

//use uuid::Uuid;
use redox_fatfs::mount;

#[cfg(target_os = "redox")]
extern "C" fn unmount_handler(_s: usize) {
    use std::sync::atomic::Ordering;
    redox_fatfs::IS_UMT.store(1, Ordering::SeqCst);
}

#[cfg(target_os = "redox")]
//set up a signal handler on redox, this implements unmounting. I have no idea what sa_flags is
//for, so I put 2. I don't think 0,0 is a valid sa_mask. I don't know what i'm doing here. When u
//send it a sigkill, it shuts off the filesystem
fn setsig() {
    use syscall::{sigaction, SigAction, SIGTERM};

    let sig_action = SigAction {
        sa_handler: unmount_handler,
        sa_mask: [0,0],
        sa_flags: 0,
    };

    sigaction(SIGTERM, Some(&sig_action), None).unwrap();
}

#[cfg(unix)]
// on linux, this is implemented properly, so no need for this unscrupulous nonsense!
fn setsig() {
    ()
}

#[cfg(unix)]
fn fork() -> isize {
    unsafe { libc::fork() as isize }
}

#[cfg(unix)]
fn pipe(pipes: &mut [i32; 2]) -> isize {
    unsafe { libc::pipe(pipes.as_mut_ptr()) as isize }
}

#[cfg(target_os = "redox")]
fn fork() -> isize {
    unsafe { syscall::Error::mux(syscall::clone(0)) as isize }
}

#[cfg(target_os = "redox")]
fn pipe(pipes: &mut [usize; 2]) -> isize {
    syscall::Error::mux(syscall::pipe2(pipes, 0)) as isize
}

fn usage() {
    println!("redox-fatfs [mountpoint_base] --uid [uid] --gid [gid] --mode [mode]");
}

/*
enum DiskId {
    Path(String),
    Uuid(Uuid),
}*/

static MOUNT_COUNT: AtomicUsize = AtomicUsize::new(0);

#[cfg(not(target_os = "redox"))]
fn disk_paths(_paths: &mut Vec<String>) {}

#[cfg(target_os = "redox")]
fn disk_paths(paths: &mut Vec<String>) {
    use std::fs;

    let mut schemes = vec![];
    match fs::read_dir(":") {
        Ok(entries) => for entry_res in entries {
            if let Ok(entry) = entry_res {
                if let Ok(path) = entry.path().into_os_string().into_string() {
                    let scheme = path.trim_left_matches(':').trim_matches('/');
                    if scheme.starts_with("disk") {
                        println!("redox-fatfs: found scheme {}", scheme);
                        schemes.push(format!("{}:", scheme));
                    }
                }
            }
        },
        Err(err) => {
            println!("redox-fatfs: failed to list schemes: {}", err);
        }
    }

    for scheme in schemes {
        match fs::read_dir(&scheme) {
            Ok(entries) => for entry_res in entries {
                if let Ok(entry) = entry_res {
                    if let Ok(path) = entry.path().into_os_string().into_string() {
                        println!("redox-fatfs: found path {}", path);
                        paths.push(path);
                    }
                }
            },
            Err(err) => {
                println!("redox-fatfs: failed to list '{}': {}", scheme, err);
            }
        }
    }
}

fn daemon(path: &str, mountpoint: &str, mut write: File, uid: u32, gid: u32, mode: u16) -> ! {
    setsig();

    println!("redox-fatfs: opening {}", path);
    match std::fs::OpenOptions::new().read(true).write(true).open(path) {
            Ok(disk) => match redox_fatfs::FileSystem::from_offset(0, disk) {
                Ok(filesystem) => {
                    println!("redox-fatfs: opened filesystem on {}", path);

                    /*let matches = if let Some(uuid) = uuid_opt {
                        if &filesystem.header.1.uuid == uuid.as_bytes() {
                            println!("redoxfs: filesystem on {} matches uuid {}", path, uuid.hyphenated());
                            true
                        } else {
                            println!("redoxfs: filesystem on {} does not match uuid {}", path, uuid.hyphenated());
                            false
                        }
                    } else {
                        true
                    };*/
                    match mount(filesystem, &mountpoint, || {
                        println!("redoxfs: mounted filesystem on {} to {}", path, mountpoint);
                        let _ = write.write(&[0]);
                    }, mode, uid, gid) {
                        Ok(()) => {
                            process::exit(0);
                        },
                        Err(err) => {
                            println!("redoxfs: failed to mount {} to {}: {}", path, mountpoint, err);
                        }
                    }

                },
                Err(err) => println!("redoxfs: failed to open filesystem {}: {}", path, err)
            },
            Err(err) => println!("redoxfs: failed to open image {}: {}", path, err)
    }



     println!("redoxfs: not able to mount path {}", path);


    let _ = write.write(&[1]);
    process::exit(1);
}

fn main() {
    let mut args = env::args().skip(1);

    /*let disk_id = match args.next() {
        Some(arg) => if arg == "--uuid" {
            let uuid = match args.next() {
                Some(arg) => match Uuid::parse_str(&arg) {
                    Ok(uuid) => uuid,
                    Err(err) => {
                        println!("redoxfs: invalid uuid '{}': {}", arg, err);
                        usage();
                        process::exit(1);
                    }
                },
                None => {
                    println!("redoxfs: no uuid provided");
                    usage();
                    process::exit(1);
                }
            };

            DiskId::Uuid(uuid)
        } else {
            DiskId::Path(arg)
        },
        None => {
            println!("redoxfs: no disk provided");
            usage();
            process::exit(1);
        }
    };*/

    let mountpoint_base = match args.next() {
        Some(arg) => arg,
        None => {
            println!("redox-fatfs: no mountpoint base provided");
            usage();
            process::exit(1);
        }
    };

    let uid = match args.next() {
        Some(arg) => {
            if arg == "--uid" {
                let uid = match args.next() {
                    Some(u) => match u.parse::<u32>() {
                        Ok(i) => i,
                        Err(e) =>  {
                            println!("redoxfs: invalid uid '{}': {}", u, e);
                            usage();
                            process::exit(1);
                        }
                    },
                    None => {
                        println!("redoxfs: no uid provided, defaulting to 0");
                        0
                    }
                };
                uid
            } else {
                println!("redoxfs: no uid provided, defaulting to 0");
                0
            }
        },
        None => {
            println!("redoxfs: no uid provided, defaulting to 0");
            0
        }
    };

    let gid = match args.next() {
        Some(arg) => {
            if arg == "--gid" {
                let uid = match args.next() {
                    Some(u) => match u.parse::<u32>() {
                        Ok(i) => i,
                        Err(e) =>  {
                            println!("redoxfs: invalid gid '{}': {}", u, e);
                            usage();
                            process::exit(1);
                        }
                    },
                    None => {
                        println!("redoxfs: no gid provided, defaulting to 0");
                        0
                    }
                };
                uid
            } else {
                println!("redoxfs: no gid provided, defaulting to 0");
                0
            }
        },
        None => {
            println!("redoxfs: no gid provided, defaulting to 0");
            0
        }
    };

    let mode = match args.next() {
        Some(arg) => {
            if arg == "--mode" {
                let uid = match args.next() {
                    Some(u) => match u.parse::<u16>() {
                        Ok(i) => i,
                        Err(e) =>  {
                            println!("redoxfs: invalid gid '{}': {}", u, e);
                            usage();
                            process::exit(1);
                        }
                    },
                    None => {
                        println!("redoxfs: no mode provided, defaulting to 0o777");
                        0o777
                    }
                };
                uid
            } else {
                println!("redoxfs: no gid provided, defaulting to 0o777");
                0o777
            }
        },
        None => {
            println!("redoxfs: no gid provided, defaulting to 0o777");
            0o777
        }
    };


    let mut paths = vec![];
    disk_paths(&mut paths);

    for path in paths {
        let mut pipes = [0; 2];
        if pipe(&mut pipes) == 0 {
            let mut read = unsafe { File::from_raw_fd(pipes[0]) };
            let write = unsafe { File::from_raw_fd(pipes[1]) };

            let pid = fork();
            if pid == 0 {
                drop(read);
                let id = MOUNT_COUNT.fetch_add(1, Ordering::SeqCst).to_string();
                let mut mount_point = mountpoint_base.clone();
                mount_point.push_str(&id);
                daemon(&path, &mount_point, write, uid, gid, mode);
            } else if pid > 0 {
                drop(write);

                let mut res = [0];
                read.read(&mut res).unwrap();

                process::exit(res[0] as i32);
            } else {
                panic!("redoxfs: failed to fork");
            }
        } else {
            panic!("redoxfs: failed to create pipe");
        }
    }
}
