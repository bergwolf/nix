use nix::sys::uio::*;
use nix::unistd::*;
use rand::{thread_rng, Rng};
use std::{cmp, iter};
use std::fs::{OpenOptions};
use std::os::unix::io::AsRawFd;

use tempdir::TempDir;
use tempfile::tempfile;

#[test]
fn test_writev() {
    let mut to_write = Vec::with_capacity(16 * 128);
    for _ in 0..16 {
        let s: String = thread_rng().gen_ascii_chars().take(128).collect();
        let b = s.as_bytes();
        to_write.extend(b.iter().map(|x| x.clone()));
    }
    // Allocate and fill iovecs
    let mut iovecs = Vec::new();
    let mut consumed = 0;
    while consumed < to_write.len() {
        let left = to_write.len() - consumed;
        let slice_len = if left <= 64 { left } else { thread_rng().gen_range(64, cmp::min(256, left)) };
        let b = &to_write[consumed..consumed+slice_len];
        iovecs.push(IoVec::from_slice(b));
        consumed += slice_len;
    }
    let pipe_res = pipe();
    assert!(pipe_res.is_ok());
    let (reader, writer) = pipe_res.ok().unwrap();
    // FileDesc will close its filedesc (reader).
    let mut read_buf: Vec<u8> = iter::repeat(0u8).take(128 * 16).collect();
    // Blocking io, should write all data.
    let write_res = writev(writer, &iovecs);
    // Successful write
    assert!(write_res.is_ok());
    let written = write_res.ok().unwrap();
    // Check whether we written all data
    assert_eq!(to_write.len(), written);
    let read_res = read(reader, &mut read_buf[..]);
    // Successful read
    assert!(read_res.is_ok());
    let read = read_res.ok().unwrap() as usize;
    // Check we have read as much as we written
    assert_eq!(read, written);
    // Check equality of written and read data
    assert_eq!(&to_write, &read_buf);
    let close_res = close(writer);
    assert!(close_res.is_ok());
    let close_res = close(reader);
    assert!(close_res.is_ok());
}

#[test]
fn test_readv() {
    let s:String = thread_rng().gen_ascii_chars().take(128).collect();
    let to_write = s.as_bytes().to_vec();
    let mut storage = Vec::new();
    let mut allocated = 0;
    while allocated < to_write.len() {
        let left = to_write.len() - allocated;
        let vec_len = if left <= 64 { left } else { thread_rng().gen_range(64, cmp::min(256, left)) };
        let v: Vec<u8> = iter::repeat(0u8).take(vec_len).collect();
        storage.push(v);
        allocated += vec_len;
    }
    let mut iovecs = Vec::with_capacity(storage.len());
    for v in storage.iter_mut() {
        iovecs.push(IoVec::from_mut_slice(&mut v[..]));
    }
    let pipe_res = pipe();
    assert!(pipe_res.is_ok());
    let (reader, writer) = pipe_res.ok().unwrap();
    // Blocking io, should write all data.
    let write_res = write(writer, &to_write);
    // Successful write
    assert!(write_res.is_ok());
    let read_res = readv(reader, &mut iovecs[..]);
    assert!(read_res.is_ok());
    let read = read_res.ok().unwrap();
    // Check whether we've read all data
    assert_eq!(to_write.len(), read);
    // Cccumulate data from iovecs
    let mut read_buf = Vec::with_capacity(to_write.len());
    for iovec in iovecs.iter() {
        read_buf.extend(iovec.as_slice().iter().map(|x| x.clone()));
    }
    // Check whether iovecs contain all written data
    assert_eq!(read_buf.len(), to_write.len());
    // Check equality of written and read data
    assert_eq!(&read_buf, &to_write);
    let close_res = close(reader);
    assert!(close_res.is_ok());
    let close_res = close(writer);
    assert!(close_res.is_ok());
}

#[test]
fn test_pwrite() {
    use std::io::Read;

    let mut file = tempfile().unwrap();
    let buf = [1u8;8];
    assert_eq!(Ok(8), pwrite(file.as_raw_fd(), &buf, 8));
    let mut file_content = Vec::new();
    file.read_to_end(&mut file_content).unwrap();
    let mut expected = vec![0u8;8];
    expected.extend(vec![1;8]);
    assert_eq!(file_content, expected);
}

#[test]
fn test_pread() {
    use std::io::Write;

    let tempdir = TempDir::new("nix-test_pread").unwrap();

    let path = tempdir.path().join("pread_test_file");
    let mut file = OpenOptions::new().write(true).read(true).create(true)
                                    .truncate(true).open(path).unwrap();
    let file_content: Vec<u8> = (0..64).collect();
    file.write_all(&file_content).unwrap();

    let mut buf = [0u8;16];
    assert_eq!(Ok(16), pread(file.as_raw_fd(), &mut buf, 16));
    let expected: Vec<_> = (16..32).collect();
    assert_eq!(&buf[..], &expected[..]);
}

#[test]
#[cfg(feature = "preadv_pwritev")]
fn test_pwritev() {
    use std::io::Read;

    let to_write: Vec<u8> = (0..128).collect();
    let expected: Vec<u8> = [vec![0;100], to_write.clone()].concat();

    let iovecs = [
        IoVec::from_slice(&to_write[0..17]),
        IoVec::from_slice(&to_write[17..64]),
        IoVec::from_slice(&to_write[64..128]),
    ];

    let tempdir = TempDir::new("nix-test_pwritev").unwrap();

    // pwritev them into a temporary file
    let path = tempdir.path().join("pwritev_test_file");
    let mut file = OpenOptions::new().write(true).read(true).create(true)
                                    .truncate(true).open(path).unwrap();

    let written = pwritev(file.as_raw_fd(), &iovecs, 100).ok().unwrap();
    assert_eq!(written, to_write.len());

    // Read the data back and make sure it matches
    let mut contents = Vec::new();
    file.read_to_end(&mut contents).unwrap();
    assert_eq!(contents, expected);
}

#[test]
#[cfg(feature = "preadv_pwritev")]
fn test_preadv() {
    use std::io::Write;

    let to_write: Vec<u8> = (0..200).collect();
    let expected: Vec<u8> = (100..200).collect();

    let tempdir = TempDir::new("nix-test_preadv").unwrap();

    let path = tempdir.path().join("preadv_test_file");

    let mut file = OpenOptions::new().read(true).write(true).create(true)
                                    .truncate(true).open(path).unwrap();
    file.write_all(&to_write).unwrap();

    let mut buffers: Vec<Vec<u8>> = vec![
        vec![0; 24],
        vec![0; 1],
        vec![0; 75],
    ];

    {
        // Borrow the buffers into IoVecs and preadv into them
        let mut iovecs: Vec<_> = buffers.iter_mut().map(
            |buf| IoVec::from_mut_slice(&mut buf[..])).collect();
        assert_eq!(Ok(100), preadv(file.as_raw_fd(), &mut iovecs, 100));
    }

    let all = buffers.concat();
    assert_eq!(all, expected);
}
