use std::{io::Write as _, path::Path};

use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader, BufWriter};

const IO_BUFFER_SIZE: usize = 128 * 1024;

pub async fn write(
    path: impl AsRef<Path>,
    content: impl AsRef<[u8]>,
) -> Result<(), std::io::Error> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    let file = tokio::fs::File::create(path).await?;
    let mut writer = BufWriter::with_capacity(IO_BUFFER_SIZE, file);
    writer.write_all(content.as_ref()).await?;
    writer.flush().await
}

/// Synchronous write for use in rayon / non-async contexts.
pub fn write_sync(path: impl AsRef<Path>, content: &[u8]) -> Result<(), std::io::Error> {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = std::fs::File::create(path)?;
    let mut writer = std::io::BufWriter::with_capacity(IO_BUFFER_SIZE, file);
    writer.write_all(content)?;
    writer.flush()
}

pub async fn read_to_string(path: impl AsRef<Path>) -> Result<String, std::io::Error> {
    let file = tokio::fs::File::open(path).await?;
    let mut reader = BufReader::with_capacity(IO_BUFFER_SIZE, file);
    let mut buf = String::new();
    reader.read_to_string(&mut buf).await?;
    Ok(buf)
}
