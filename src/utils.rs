pub async fn write(
    path: impl AsRef<std::path::Path>,
    content: impl AsRef<[u8]>,
) -> Result<(), std::io::Error> {
    // create parent directories if they don't exist
    if let Some(parent) = path.as_ref().parent() {
        tokio::fs::create_dir_all(parent).await?;
    }
    tokio::fs::write(path, content).await
}

pub async fn read_to_string(path: impl AsRef<std::path::Path>) -> Result<String, std::io::Error> {
    tokio::fs::read_to_string(path).await
}
