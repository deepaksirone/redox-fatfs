use Result;
use std::io::ErrorKind;



pub fn from<T>(res: Result<T>) -> syscall::Result<T> {
    match res {
        Ok(s) => Ok(s),
        Err(e) => {
             match e.kind() {
                 ErrorKind::NotFound => Err(syscall::Error::new(syscall::ENOENT)),
                 ErrorKind::InvalidInput | ErrorKind::InvalidData => Err(syscall::Error::new(syscall::EINVAL)),
                 ErrorKind::PermissionDenied => Err(syscall::Error::new(syscall::EPERM)),
                 ErrorKind::AlreadyExists => Err(syscall::Error::new(syscall::EINVAL)),
                 _ => Err(syscall::Error::new(syscall::EIO))
             }
        }
    }
}
