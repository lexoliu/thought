//! Platform-specific batched I/O backend.
//!
//! Provides `batch_read_files` and `batch_write_files` that operate on raw
//! `Vec<u8>` with **zero UTF-8 validation** in the I/O layer.
//!
//! - **Linux**: uses `io_uring` for kernel-side batched I/O.
//! - **macOS / other**: uses `rayon` parallel iterators over `std::fs`.

use std::{
    io,
    path::{Path, PathBuf},
};

/// Read many files in parallel, returning raw bytes (no UTF-8 validation).
pub fn batch_read_files(paths: &[PathBuf]) -> io::Result<Vec<(PathBuf, Vec<u8>)>> {
    imp::batch_read_files(paths)
}

/// Write many files in parallel, creating parent directories as needed.
pub fn batch_write_files<P, D>(entries: &[(P, D)]) -> io::Result<()>
where
    P: AsRef<Path> + Sync,
    D: AsRef<[u8]> + Sync,
{
    imp::batch_write_files(entries)
}

// ── Linux: io_uring ──────────────────────────────────────────────────────────

#[cfg(target_os = "linux")]
mod imp {
    use std::{
        fs,
        io,
        os::unix::io::AsRawFd,
        path::{Path, PathBuf},
    };

    use io_uring::{IoUring, opcode, types};

    /// Maximum number of SQEs submitted per batch.
    const RING_SIZE: u32 = 256;

    pub fn batch_read_files(paths: &[PathBuf]) -> io::Result<Vec<(PathBuf, Vec<u8>)>> {
        if paths.is_empty() {
            return Ok(Vec::new());
        }

        let mut ring = IoUring::new(RING_SIZE)?;
        let mut results: Vec<(PathBuf, Vec<u8>)> = Vec::with_capacity(paths.len());

        // Process in chunks that fit the ring.
        for chunk in paths.chunks(RING_SIZE as usize) {
            // Open files and allocate buffers.
            let mut opened: Vec<(PathBuf, fs::File, Vec<u8>)> = Vec::with_capacity(chunk.len());
            for path in chunk {
                let file = fs::File::open(path)?;
                let len = file.metadata()?.len() as usize;
                let buf = vec![0u8; len];
                opened.push((path.clone(), file, buf));
            }

            // Submit read SQEs.
            unsafe {
                for (idx, (_path, file, buf)) in opened.iter_mut().enumerate() {
                    let entry = opcode::Read::new(
                        types::Fd(file.as_raw_fd()),
                        buf.as_mut_ptr(),
                        buf.len() as u32,
                    )
                    .offset(0)
                    .build()
                    .user_data(idx as u64);
                    ring.submission().push(&entry).map_err(|_| {
                        io::Error::new(io::ErrorKind::Other, "io_uring submission queue full")
                    })?;
                }
            }

            ring.submit_and_wait(opened.len())?;

            // Reap completions.
            for cqe in ring.completion() {
                let idx = cqe.user_data() as usize;
                let ret = cqe.result();
                if ret < 0 {
                    return Err(io::Error::from_raw_os_error(-ret));
                }
                // Truncate buffer to actual bytes read.
                opened[idx].2.truncate(ret as usize);
            }

            for (path, _file, buf) in opened {
                results.push((path, buf));
            }
        }

        Ok(results)
    }

    pub fn batch_write_files<P, D>(entries: &[(P, D)]) -> io::Result<()>
    where
        P: AsRef<Path> + Sync,
        D: AsRef<[u8]> + Sync,
    {
        if entries.is_empty() {
            return Ok(());
        }

        let mut ring = IoUring::new(RING_SIZE)?;

        for chunk in entries.chunks(RING_SIZE as usize) {
            // Create parent dirs and open files for writing.
            let mut opened: Vec<fs::File> = Vec::with_capacity(chunk.len());
            for (path, _data) in chunk {
                let path = path.as_ref();
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                opened.push(fs::File::create(path)?);
            }

            // Submit write SQEs.
            unsafe {
                for (idx, ((_path, data), file)) in
                    chunk.iter().zip(opened.iter()).enumerate()
                {
                    let bytes = data.as_ref();
                    let entry = opcode::Write::new(
                        types::Fd(file.as_raw_fd()),
                        bytes.as_ptr(),
                        bytes.len() as u32,
                    )
                    .offset(0)
                    .build()
                    .user_data(idx as u64);
                    ring.submission().push(&entry).map_err(|_| {
                        io::Error::new(io::ErrorKind::Other, "io_uring submission queue full")
                    })?;
                }
            }

            ring.submit_and_wait(chunk.len())?;

            for cqe in ring.completion() {
                let ret = cqe.result();
                if ret < 0 {
                    return Err(io::Error::from_raw_os_error(-ret));
                }
            }
        }

        Ok(())
    }
}

// ── macOS / fallback: rayon + std::fs ────────────────────────────────────────

#[cfg(not(target_os = "linux"))]
mod imp {
    use std::{
        fs,
        io,
        path::{Path, PathBuf},
    };

    use rayon::prelude::*;

    pub fn batch_read_files(paths: &[PathBuf]) -> io::Result<Vec<(PathBuf, Vec<u8>)>> {
        paths
            .par_iter()
            .map(|p| {
                let data = fs::read(p)?;
                Ok((p.clone(), data))
            })
            .collect()
    }

    pub fn batch_write_files<P, D>(entries: &[(P, D)]) -> io::Result<()>
    where
        P: AsRef<Path> + Sync,
        D: AsRef<[u8]> + Sync,
    {
        entries
            .par_iter()
            .try_for_each(|(path, data)| {
                let path = path.as_ref();
                if let Some(parent) = path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(path, data.as_ref())
            })
    }
}
