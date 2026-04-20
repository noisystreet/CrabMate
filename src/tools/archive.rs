//! 归档工具（压缩/解压）

use std::io::{Read, Write};
use std::path::Path;

use crate::tools::ToolContext;

/// 创建归档
pub fn archive_pack(args_json: &str, working_dir: &Path, _ctx: &ToolContext<'_>) -> String {
    let args: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误: {}", e),
    };

    let output = args
        .get("output")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let sources = args
        .get("sources")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if output.is_empty() {
        return "错误: 缺少 output 参数".to_string();
    }
    if sources.is_empty() {
        return "错误: 缺少 sources 参数".to_string();
    }

    let output_path = working_dir.join(output);

    // 根据扩展名检测格式
    let format = detect_format(output);

    // 构建命令
    let result = match format.as_str() {
        "zip" => pack_zip(working_dir, &output_path, &sources),
        "tar" => pack_tar(working_dir, &output_path, &sources, None),
        "tar.gz" | "tgz" => pack_tar(working_dir, &output_path, &sources, Some("gzip")),
        "tar.bz2" | "tbz2" => pack_tar(working_dir, &output_path, &sources, Some("bzip2")),
        "tar.xz" | "txz" => pack_tar(working_dir, &output_path, &sources, Some("xz")),
        _ => return format!("不支持的归档格式: {}", output),
    };

    match result {
        Ok(()) => format!("已创建归档: {}", output),
        Err(e) => format!("创建归档失败: {}", e),
    }
}

/// 解压归档
pub fn archive_unpack(args_json: &str, working_dir: &Path, _ctx: &ToolContext<'_>) -> String {
    let args: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误: {}", e),
    };

    let archive = args
        .get("archive")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let output_dir = args
        .get("output_dir")
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    if archive.is_empty() {
        return "错误: 缺少 archive 参数".to_string();
    }

    let archive_path = working_dir.join(archive);
    if !archive_path.exists() {
        return format!("归档文件不存在: {}", archive);
    }

    let output_path = working_dir.join(output_dir);

    // 根据扩展名检测格式
    let format = detect_format(archive);

    let result = match format.as_str() {
        "zip" => unpack_zip(&archive_path, &output_path),
        "tar" => unpack_tar(&archive_path, &output_path, None),
        "tar.gz" | "tgz" => unpack_tar(&archive_path, &output_path, Some("gzip")),
        "tar.bz2" | "tbz2" => unpack_tar(&archive_path, &output_path, Some("bzip2")),
        "tar.xz" | "txz" => unpack_tar(&archive_path, &output_path, Some("xz")),
        "7z" => unpack_7z(&archive_path, &output_path),
        "rar" => unpack_rar(&archive_path, &output_path),
        _ => return format!("不支持的归档格式: {}", archive),
    };

    match result {
        Ok(files) => format!("已解压 {} 个文件到: {}", files, output_dir),
        Err(e) => format!("解压失败: {}", e),
    }
}

/// 列出归档内容
pub fn archive_list(args_json: &str, working_dir: &Path, _ctx: &ToolContext<'_>) -> String {
    let args: serde_json::Value = match serde_json::from_str(args_json) {
        Ok(v) => v,
        Err(e) => return format!("参数解析错误: {}", e),
    };

    let archive = args
        .get("archive")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    let verbose = args
        .get("verbose")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if archive.is_empty() {
        return "错误: 缺少 archive 参数".to_string();
    }

    let archive_path = working_dir.join(archive);
    if !archive_path.exists() {
        return format!("归档文件不存在: {}", archive);
    }

    // 根据扩展名检测格式
    let format = detect_format(archive);

    let result = match format.as_str() {
        "zip" => list_zip(&archive_path, verbose),
        "tar" | "tar.gz" | "tgz" | "tar.bz2" | "tbz2" | "tar.xz" | "txz" => {
            list_tar(&archive_path, verbose)
        }
        "7z" => list_7z(&archive_path, verbose),
        "rar" => list_rar(&archive_path, verbose),
        _ => return format!("不支持的归档格式: {}", archive),
    };

    match result {
        Ok(list) => list,
        Err(e) => format!("列出内容失败: {}", e),
    }
}

// ============ 辅助函数 ============

fn detect_format(filename: &str) -> String {
    let lower = filename.to_lowercase();
    if lower.ends_with(".tar.gz") || lower.ends_with(".tgz") {
        "tar.gz".to_string()
    } else if lower.ends_with(".tar.bz2") || lower.ends_with(".tbz2") {
        "tar.bz2".to_string()
    } else if lower.ends_with(".tar.xz") || lower.ends_with(".txz") {
        "tar.xz".to_string()
    } else if lower.ends_with(".tar") {
        "tar".to_string()
    } else if lower.ends_with(".zip") {
        "zip".to_string()
    } else if lower.ends_with(".7z") {
        "7z".to_string()
    } else if lower.ends_with(".rar") {
        "rar".to_string()
    } else {
        "unknown".to_string()
    }
}

// ============ 打包实现 ============

fn pack_zip(
    working_dir: &Path,
    output: &Path,
    sources: &[String],
) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::create(output)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for source in sources {
        let source_path = working_dir.join(source);
        if source_path.is_file() {
            let name = source_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(source);
            zip.start_file(name, options)?;
            let content = std::fs::read(&source_path)?;
            zip.write_all(&content)?;
        } else if source_path.is_dir() {
            add_dir_to_zip(&mut zip, &source_path, source, options)?;
        }
    }

    zip.finish()?;
    Ok(())
}

fn add_dir_to_zip(
    zip: &mut zip::ZipWriter<std::fs::File>,
    dir: &Path,
    _base_name: &str,
    options: zip::write::SimpleFileOptions,
) -> Result<(), Box<dyn std::error::Error>> {
    for entry in walkdir::WalkDir::new(dir) {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            let relative_path = path.strip_prefix(dir.parent().unwrap_or(dir))?;
            zip.start_file(relative_path.to_string_lossy(), options)?;
            let content = std::fs::read(path)?;
            zip.write_all(&content)?;
        }
    }
    Ok(())
}

fn pack_tar(
    working_dir: &Path,
    output: &Path,
    sources: &[String],
    compress: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    let file = std::fs::File::create(output)?;

    let enc: Box<dyn std::io::Write> = match compress {
        Some("gzip") => Box::new(flate2::write::GzEncoder::new(
            file,
            flate2::Compression::default(),
        )),
        Some("bzip2") => Box::new(bzip2::write::BzEncoder::new(
            file,
            bzip2::Compression::default(),
        )),
        Some("xz") => Box::new(xz2::write::XzEncoder::new(file, 6)),
        _ => Box::new(file),
    };

    let mut tar = tar::Builder::new(enc);

    for source in sources {
        let source_path = working_dir.join(source);
        if source_path.is_file() {
            tar.append_path_with_name(&source_path, source)?;
        } else if source_path.is_dir() {
            tar.append_dir_all(source, &source_path)?;
        }
    }

    tar.finish()?;
    Ok(())
}

// ============ 解压实现 ============

fn unpack_zip(archive: &Path, output: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let mut count = 0;

    std::fs::create_dir_all(output)?;

    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let outpath = output.join(file.name());

        if file.is_dir() {
            std::fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let mut outfile = std::fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
            count += 1;
        }
    }

    Ok(count)
}

fn unpack_tar(
    archive: &Path,
    output: &Path,
    compress: Option<&str>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(archive)?;

    let dec: Box<dyn std::io::Read> = match compress {
        Some("gzip") => Box::new(flate2::read::GzDecoder::new(file)),
        Some("bzip2") => Box::new(bzip2::read::BzDecoder::new(file)),
        Some("xz") => Box::new(xz2::read::XzDecoder::new(file)),
        _ => Box::new(file),
    };

    let mut tar = tar::Archive::new(dec);
    std::fs::create_dir_all(output)?;
    tar.unpack(output)?;

    // 统计文件数
    let count = walkdir::WalkDir::new(output)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .count();

    Ok(count)
}

fn unpack_7z(archive: &Path, output: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    // 使用 7z 命令行工具
    let output = std::process::Command::new("7z")
        .args([
            "x",
            "-o",
            &output.to_string_lossy(),
            &archive.to_string_lossy(),
        ])
        .output()?;

    if !output.status.success() {
        return Err(format!("7z 解压失败: {}", String::from_utf8_lossy(&output.stderr)).into());
    }

    // 统计文件数（简化处理）
    Ok(0)
}

fn unpack_rar(archive: &Path, output: &Path) -> Result<usize, Box<dyn std::error::Error>> {
    // 使用 unrar 命令行工具
    let output = std::process::Command::new("unrar")
        .args([
            "x",
            "-o+",
            &archive.to_string_lossy(),
            &output.to_string_lossy(),
        ])
        .output()?;

    if !output.status.success() {
        return Err(format!(
            "unrar 解压失败: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    Ok(0)
}

// ============ 列表实现 ============

fn list_zip(archive: &Path, verbose: bool) -> Result<String, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)?;

    let mut lines = vec![format!(
        "ZIP 归档: {} ({} 个文件)",
        archive.display(),
        zip.len()
    )];

    for i in 0..zip.len() {
        let file = zip.by_index(i)?;
        if verbose {
            let modified = file
                .last_modified()
                .map(|dt| format!("{:?}", dt))
                .unwrap_or_else(|| "unknown".to_string());
            lines.push(format!(
                "  {} ({} bytes, {})",
                file.name(),
                file.size(),
                modified
            ));
        } else {
            lines.push(format!("  {}", file.name()));
        }
    }

    Ok(lines.join("\n"))
}

fn list_tar(archive: &Path, verbose: bool) -> Result<String, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(archive)?;
    let mut lines = vec![format!("TAR 归档: {}", archive.display())];

    // 尝试检测压缩格式
    let mut buf = [0u8; 2];
    let mut file = file;
    file.read_exact(&mut buf)?;
    let file = std::fs::File::open(archive)?;

    let dec: Box<dyn std::io::Read> = if buf[0] == 0x1f && buf[1] == 0x8b {
        Box::new(flate2::read::GzDecoder::new(file))
    } else if buf[0] == b'B' && buf[1] == b'Z' {
        Box::new(bzip2::read::BzDecoder::new(file))
    } else if buf[0] == 0xfd && buf[1] == 0x37 {
        Box::new(xz2::read::XzDecoder::new(file))
    } else {
        Box::new(file)
    };

    let mut tar = tar::Archive::new(dec);
    let mut count = 0;

    for entry in tar.entries()? {
        let entry = entry?;
        let path = entry.path()?;
        if verbose {
            lines.push(format!("  {} ({} bytes)", path.display(), entry.size()));
        } else {
            lines.push(format!("  {}", path.display()));
        }
        count += 1;
    }

    lines[0] = format!("TAR 归档: {} ({} 个文件)", archive.display(), count);
    Ok(lines.join("\n"))
}

fn list_7z(archive: &Path, _verbose: bool) -> Result<String, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("7z")
        .args(["l", &archive.to_string_lossy()])
        .output()?;

    if !output.status.success() {
        return Err(format!("7z 列表失败: {}", String::from_utf8_lossy(&output.stderr)).into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn list_rar(archive: &Path, _verbose: bool) -> Result<String, Box<dyn std::error::Error>> {
    let output = std::process::Command::new("unrar")
        .args(["l", &archive.to_string_lossy()])
        .output()?;

    if !output.status.success() {
        return Err(format!(
            "unrar 列表失败: {}",
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }

    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}
