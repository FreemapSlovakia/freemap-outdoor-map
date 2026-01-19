use std::path::{Path, PathBuf};

pub(crate) fn index_file_path(base: &Path, index_zoom: u32, x: u32, y: u32) -> PathBuf {
    let mut path = base.to_path_buf();
    path.push(index_zoom.to_string());
    path.push(x.to_string());
    path.push(format!("{y}.index"));
    path
}
