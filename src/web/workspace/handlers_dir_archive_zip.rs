//! 将工作区目录打成内存 zip（不跟随符号链接；条目名禁止 `..`）。

use std::io::{Cursor, Write};
use std::path::{Component, Path};

use walkdir::WalkDir;
use zip::ZipWriter;
use zip::write::SimpleFileOptions;

use crate::cm_web_host::http_types::limits::{
    WORKSPACE_DIR_ARCHIVE_MAX_DEPTH, WORKSPACE_DIR_ARCHIVE_MAX_ENTRIES,
    WORKSPACE_DIR_ARCHIVE_MAX_UNCOMPRESSED,
};

#[derive(Debug)]
pub(super) struct ZipDirError {
    pub code: &'static str,
    pub message: String,
}

impl ZipDirError {
    fn too_large(msg: impl Into<String>) -> Self {
        Self {
            code: "WORKSPACE_ARCHIVE_TOO_LARGE",
            message: msg.into(),
        }
    }
}

/// `root_canonical` 为工作区根；`dir_canonical` 为要打包的目录（须在根内）。
/// `zip_prefix` 为 zip 内顶层目录名（根归档为空）。
pub(super) fn zip_directory_bytes(
    root_canonical: &Path,
    dir_canonical: &Path,
    zip_prefix: &str,
) -> Result<Vec<u8>, ZipDirError> {
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let mut uncompressed: u64 = 0;
    let mut files = 0usize;
    for ent in WalkDir::new(dir_canonical)
        .follow_links(false)
        .max_depth(WORKSPACE_DIR_ARCHIVE_MAX_DEPTH)
    {
        let ent = ent.map_err(|e| ZipDirError {
            code: "WORKSPACE_ARCHIVE_WALK",
            message: format!("遍历目录失败: {e}"),
        })?;
        if !ent.file_type().is_file() {
            continue;
        }
        let ctx = AppendZipCtx {
            root: root_canonical,
            dir: dir_canonical,
            path: ent.path(),
            zip_prefix,
            files: &mut files,
            uncompressed: &mut uncompressed,
        };
        append_zip_file(&mut zip, options, ctx)?;
    }
    if files == 0 {
        return empty_dir_zip(zip, options, zip_prefix);
    }
    finish_zip(zip)
}

struct AppendZipCtx<'a> {
    root: &'a Path,
    dir: &'a Path,
    path: &'a Path,
    zip_prefix: &'a str,
    files: &'a mut usize,
    uncompressed: &'a mut u64,
}

fn append_zip_file(
    zip: &mut ZipWriter<Cursor<Vec<u8>>>,
    options: SimpleFileOptions,
    ctx: AppendZipCtx<'_>,
) -> Result<(), ZipDirError> {
    *ctx.files += 1;
    if *ctx.files > WORKSPACE_DIR_ARCHIVE_MAX_ENTRIES {
        return Err(ZipDirError::too_large(format!(
            "目录文件数超过上限 {WORKSPACE_DIR_ARCHIVE_MAX_ENTRIES}"
        )));
    }
    let remain = WORKSPACE_DIR_ARCHIVE_MAX_UNCOMPRESSED.saturating_sub(*ctx.uncompressed);
    if remain == 0 {
        return Err(ZipDirError::too_large(format!(
            "未压缩合计超过上限 {} 字节",
            WORKSPACE_DIR_ARCHIVE_MAX_UNCOMPRESSED
        )));
    }
    let rel = zip_entry_name(ctx.dir, ctx.path, ctx.zip_prefix)?;
    let bytes = read_member_under_root(ctx.root, ctx.dir, ctx.path, remain)?;
    *ctx.uncompressed = ctx.uncompressed.saturating_add(bytes.len() as u64);
    zip.start_file(&rel, options).map_err(|e| ZipDirError {
        code: "WORKSPACE_ARCHIVE_WRITE",
        message: format!("写入 zip 失败: {e}"),
    })?;
    zip.write_all(&bytes).map_err(|e| ZipDirError {
        code: "WORKSPACE_ARCHIVE_WRITE",
        message: format!("写入 zip 失败: {e}"),
    })?;
    Ok(())
}

fn read_member_under_root(
    root: &Path,
    dir: &Path,
    path: &Path,
    max_b: u64,
) -> Result<Vec<u8>, ZipDirError> {
    let canon = path.canonicalize().map_err(|e| ZipDirError {
        code: "WORKSPACE_ARCHIVE_READ",
        message: format!("读取失败: {e}"),
    })?;
    if !canon.starts_with(dir) || !canon.starts_with(root) {
        return Err(ZipDirError {
            code: "WORKSPACE_ARCHIVE_WALK",
            message: "文件不在目标目录内".to_string(),
        });
    }
    #[cfg(unix)]
    {
        super::handlers_sync::workspace_read_file_bytes_sync_unix(
            root.to_path_buf(),
            canon,
            max_b,
        )
        .map_err(|msg| {
            if msg.contains("过大") {
                ZipDirError::too_large(msg)
            } else {
                ZipDirError {
                    code: "WORKSPACE_ARCHIVE_READ",
                    message: msg,
                }
            }
        })
    }
    #[cfg(not(unix))]
    {
        let bytes = std::fs::read(&canon).map_err(|e| ZipDirError {
            code: "WORKSPACE_ARCHIVE_READ",
            message: format!("读取失败: {e}"),
        })?;
        if bytes.len() as u64 > max_b {
            return Err(ZipDirError::too_large(format!(
                "未压缩合计超过上限 {} 字节",
                WORKSPACE_DIR_ARCHIVE_MAX_UNCOMPRESSED
            )));
        }
        Ok(bytes)
    }
}

fn empty_dir_zip(
    zip: ZipWriter<Cursor<Vec<u8>>>,
    options: SimpleFileOptions,
    zip_prefix: &str,
) -> Result<Vec<u8>, ZipDirError> {
    if zip_prefix.is_empty() {
        let mut zip = zip;
        zip.add_directory("workspace/", options).map_err(|e| ZipDirError {
            code: "WORKSPACE_ARCHIVE_WRITE",
            message: format!("写入 zip 失败: {e}"),
        })?;
        return finish_zip(zip);
    }
    add_empty_prefix_dir(zip, options, zip_prefix)
}

fn add_empty_prefix_dir(
    mut zip: ZipWriter<Cursor<Vec<u8>>>,
    options: SimpleFileOptions,
    zip_prefix: &str,
) -> Result<Vec<u8>, ZipDirError> {
    zip.add_directory(format!("{zip_prefix}/"), options)
        .map_err(|e| ZipDirError {
            code: "WORKSPACE_ARCHIVE_WRITE",
            message: format!("写入 zip 失败: {e}"),
        })?;
    finish_zip(zip)
}

fn finish_zip(zip: ZipWriter<Cursor<Vec<u8>>>) -> Result<Vec<u8>, ZipDirError> {
    let cursor = zip.finish().map_err(|e| ZipDirError {
        code: "WORKSPACE_ARCHIVE_WRITE",
        message: format!("完成 zip 失败: {e}"),
    })?;
    let out = cursor.into_inner();
    if out.len() as u64 > WORKSPACE_DIR_ARCHIVE_MAX_UNCOMPRESSED {
        return Err(ZipDirError::too_large(format!(
            "zip 超过上限 {} 字节",
            WORKSPACE_DIR_ARCHIVE_MAX_UNCOMPRESSED
        )));
    }
    Ok(out)
}

fn zip_entry_name(dir: &Path, file: &Path, prefix: &str) -> Result<String, ZipDirError> {
    let rel = file.strip_prefix(dir).map_err(|_| ZipDirError {
        code: "WORKSPACE_ARCHIVE_WALK",
        message: "文件不在目标目录内".to_string(),
    })?;
    if rel.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return Err(ZipDirError {
            code: "WORKSPACE_ARCHIVE_WALK",
            message: "zip 条目路径非法".to_string(),
        });
    }
    let inner = rel
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/");
    if inner.is_empty() {
        return Err(ZipDirError {
            code: "WORKSPACE_ARCHIVE_WALK",
            message: "zip 条目为空".to_string(),
        });
    }
    if prefix.is_empty() {
        Ok(inner)
    } else {
        Ok(format!("{prefix}/{inner}"))
    }
}

/// zip 内顶层目录名 / 下载 stem；空表示打包工作区根。
pub(super) fn archive_dir_stem(rel_dir: &str) -> &str {
    let t = rel_dir.trim().trim_matches('/');
    let stem = t.rsplit(['/', '\\']).next().unwrap_or(t);
    if stem.is_empty() || stem == "." || stem == ".." {
        ""
    } else {
        stem
    }
}

pub(super) fn archive_zip_filename(rel_dir: &str) -> String {
    let stem = archive_dir_stem(rel_dir);
    if stem.is_empty() {
        "workspace.zip".to_string()
    } else {
        format!("{stem}.zip")
    }
}

#[cfg(test)]
mod tests {
    use super::{archive_dir_stem, archive_zip_filename, zip_directory_bytes, zip_entry_name};
    use std::path::PathBuf;

    #[test]
    fn archive_filename_uses_last_segment() {
        assert_eq!(archive_zip_filename("notes"), "notes.zip");
        assert_eq!(archive_zip_filename("a/b"), "b.zip");
        assert_eq!(archive_zip_filename(""), "workspace.zip");
        assert_eq!(archive_dir_stem("notes.zip"), "notes.zip");
        assert_eq!(archive_zip_filename("notes.zip"), "notes.zip.zip");
    }

    #[test]
    fn zip_entry_joins_prefix() {
        let dir = PathBuf::from("/ws/a");
        let file = PathBuf::from("/ws/a/x.txt");
        let n = zip_entry_name(&dir, &file, "a").expect("name");
        assert_eq!(n, "a/x.txt");
    }

    #[test]
    fn zip_empty_dir_is_valid_archive() {
        let root = tempfile::tempdir().expect("tmp");
        let dir = root.path().join("empty");
        std::fs::create_dir(&dir).expect("mkdir");
        let bytes = zip_directory_bytes(&dir, &dir, "empty").expect("zip");
        assert!(!bytes.is_empty());
    }
}
