//! 归档工具（压缩/解压）

use std::collections::BTreeSet;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use crate::cm_tools::tools::ToolContext;
use crate::cm_tools::tools::tool_param_types::{ArchiveListArgs, ArchivePackArgs, ArchiveUnpackArgs};

/// 超大归档列表默认最多输出的条目数（避免灌满上下文）。
const ARCHIVE_LIST_MAX_ENTRIES: usize = 250;

/// 按最长后缀优先匹配（须先 `.tar.gz` 再 `.tar`）。
const ARCHIVE_FORMAT_SUFFIXES: &[(&str, &str)] = &[
    (".tar.gz", "tar.gz"),
    (".tgz", "tar.gz"),
    (".tar.bz2", "tar.bz2"),
    (".tbz2", "tar.bz2"),
    (".tar.xz", "tar.xz"),
    (".txz", "tar.xz"),
    (".tar", "tar"),
    (".zip", "zip"),
    (".7z", "7z"),
    (".rar", "rar"),
];

/// 创建归档
pub fn archive_pack(args_json: &str, working_dir: &Path, _ctx: &ToolContext<'_>) -> String {
    let v = match crate::cm_tools::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: ArchivePackArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数解析错误: {}", e),
    };

    let output = args.output.trim();
    let sources: Vec<String> = args
        .sources
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    let _ = (&args.exclude, &args.format);

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
    let v = match crate::cm_tools::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: ArchiveUnpackArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数解析错误: {}", e),
    };

    let archive = args.archive.trim();
    let output_dir = args.output_dir.trim();
    let strip_components = args.strip_components.unwrap_or(0);
    let selected_files: Vec<String> = args
        .files
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();

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

    let unpack_opts = UnpackOptions {
        strip_components,
        selected_files: selected_files.as_slice(),
    };

    let result = match format.as_str() {
        "zip" => unpack_zip(&archive_path, &output_path, &unpack_opts),
        "tar" => unpack_tar(&archive_path, &output_path, None, &unpack_opts),
        "tar.gz" | "tgz" => unpack_tar(&archive_path, &output_path, Some("gzip"), &unpack_opts),
        "tar.bz2" | "tbz2" => unpack_tar(&archive_path, &output_path, Some("bzip2"), &unpack_opts),
        "tar.xz" | "txz" => unpack_tar(&archive_path, &output_path, Some("xz"), &unpack_opts),
        "7z" => unpack_7z(&archive_path, &output_path),
        "rar" => unpack_rar(&archive_path, &output_path),
        _ => return format!("不支持的归档格式: {}", archive),
    };

    match result {
        Ok(files) => format_unpack_success(files, output_dir, &output_path, strip_components),
        Err(e) => format!("解压失败: {}", e),
    }
}

/// 列出归档内容
pub fn archive_list(args_json: &str, working_dir: &Path, _ctx: &ToolContext<'_>) -> String {
    let v = match crate::cm_tools::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: ArchiveListArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数解析错误: {}", e),
    };

    let archive = args.archive.trim();
    let verbose = args.verbose;
    let max_entries = args
        .max_entries
        .map(|n| n as usize)
        .unwrap_or(ARCHIVE_LIST_MAX_ENTRIES)
        .max(1);

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
        "zip" => list_zip(&archive_path, verbose, max_entries),
        "tar" | "tar.gz" | "tgz" | "tar.bz2" | "tbz2" | "tar.xz" | "txz" => {
            list_tar(&archive_path, verbose, max_entries)
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

struct UnpackOptions<'a> {
    strip_components: u32,
    selected_files: &'a [String],
}

fn format_unpack_success(
    files: usize,
    output_dir: &str,
    output_path: &Path,
    strip_components: u32,
) -> String {
    let mut out = format!("已解压 {} 个文件到: {}", files, output_dir);
    if let Some(tops) = read_immediate_child_names(output_path)
        && !tops.is_empty()
    {
        out.push_str(&format!("\n顶层条目: {}", tops.join(", ")));
    }
    if output_dir != "." && output_dir != "./" {
        out.push_str(
            "\n提示: 源码包通常可直接 `output_dir=\".\"` 解压到工作区根；勿自创嵌套目录名以免路径加深。",
        );
    }
    if strip_components > 0 {
        out.push_str(&format!(
            "\n已剥离路径前缀 {} 层（strip_components={strip_components}）。",
            strip_components
        ));
    } else if output_dir == "." || output_dir == "./" {
        out.push_str(
            "\n若归档内仅有一层根目录且需扁平化，可设 `strip_components=1`；否则保持默认即可。",
        );
    }
    out
}

fn read_immediate_child_names(dir: &Path) -> Option<Vec<String>> {
    let mut names = Vec::new();
    let entries = std::fs::read_dir(dir).ok()?;
    for ent in entries.flatten() {
        let name = ent.file_name().to_string_lossy().to_string();
        if !name.is_empty() {
            names.push(name);
        }
    }
    names.sort();
    Some(names)
}

fn strip_archive_path(path: &Path, strip_components: u32) -> Option<PathBuf> {
    let comps: Vec<_> = path
        .components()
        .filter(|c| !matches!(c, Component::CurDir))
        .collect();
    if comps.len() <= strip_components as usize {
        return None;
    }
    Some(comps[strip_components as usize..].iter().collect())
}

fn archive_entry_selected(path: &Path, selected_files: &[String]) -> bool {
    if selected_files.is_empty() {
        return true;
    }
    let normalized = path.to_string_lossy().replace('\\', "/");
    selected_files.iter().any(|f| {
        let f = f.trim_matches('/');
        normalized == f || normalized.starts_with(&format!("{f}/"))
    })
}

fn collect_top_level_prefixes(paths: &[String]) -> Vec<String> {
    let mut set = BTreeSet::new();
    for p in paths {
        if let Some(first) = p.split('/').next().filter(|s| !s.is_empty()) {
            set.insert(first.to_string());
        } else if !p.is_empty() {
            set.insert(p.clone());
        }
    }
    set.into_iter().collect()
}

fn format_capped_list(
    header: &str,
    total: usize,
    max_entries: usize,
    mut lines: Vec<String>,
    all_paths: &[String],
) -> String {
    if total <= max_entries {
        let mut out = vec![header.to_string()];
        out.extend(lines);
        return out.join("\n");
    }
    lines.truncate(max_entries);
    let tops = collect_top_level_prefixes(all_paths);
    let top_summary = if tops.is_empty() {
        String::new()
    } else {
        format!("\n顶层目录/文件（{} 项）: {}", tops.len(), tops.join(", "))
    };
    let mut out = vec![format!(
        "{header}\n（已截断：共 {total} 项，仅显示前 {max_entries} 项；超大归档建议先看顶层再解压。{top_summary}）"
    )];
    out.extend(lines);
    out.join("\n")
}

// ============ 辅助函数 ============

fn detect_format(filename: &str) -> String {
    let lower = filename.to_lowercase();
    ARCHIVE_FORMAT_SUFFIXES
        .iter()
        .find(|(suffix, _)| lower.ends_with(suffix))
        .map(|(_, kind)| (*kind).to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn tar_magic_kind(magic: [u8; 2]) -> &'static str {
    match &magic {
        b"\x1f\x8b" => "gzip",
        b"BZ" => "bzip2",
        b"\xfd\x37" => "xz",
        _ => "none",
    }
}

fn tar_decoder_from_magic(file: std::fs::File, magic: [u8; 2]) -> Box<dyn std::io::Read> {
    match tar_magic_kind(magic) {
        "gzip" => Box::new(flate2::read::GzDecoder::new(file)),
        "bzip2" => Box::new(bzip2::read::BzDecoder::new(file)),
        "xz" => Box::new(xz2::read::XzDecoder::new(file)),
        _ => Box::new(file),
    }
}

fn open_tar_list_decoder(
    archive: &Path,
) -> Result<Box<dyn std::io::Read>, Box<dyn std::error::Error>> {
    let mut probe = std::fs::File::open(archive)?;
    let mut magic = [0u8; 2];
    probe.read_exact(&mut magic)?;
    let file = std::fs::File::open(archive)?;
    Ok(tar_decoder_from_magic(file, magic))
}

fn tar_decoder_from_compress(
    file: std::fs::File,
    compress: Option<&str>,
) -> Box<dyn std::io::Read> {
    match compress {
        Some("gzip") => Box::new(flate2::read::GzDecoder::new(file)),
        Some("bzip2") => Box::new(bzip2::read::BzDecoder::new(file)),
        Some("xz") => Box::new(xz2::read::XzDecoder::new(file)),
        _ => Box::new(file),
    }
}

fn format_archive_list_line(name: &str, verbose: bool, verbose_detail: &str) -> String {
    if verbose {
        format!("  {name} ({verbose_detail})")
    } else {
        format!("  {name}")
    }
}

fn maybe_push_list_line(lines: &mut Vec<String>, max_entries: usize, line: String) {
    if lines.len() < max_entries {
        lines.push(line);
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

fn zip_member_outpath(name: &str, opts: &UnpackOptions<'_>, output: &Path) -> Option<PathBuf> {
    let name = PathBuf::from(name);
    if !archive_entry_selected(&name, opts.selected_files) {
        return None;
    }
    let rel = strip_archive_path(&name, opts.strip_components)?;
    Some(output.join(rel))
}

fn write_member_to_disk(
    is_dir: bool,
    reader: &mut impl Read,
    outpath: &Path,
) -> Result<usize, Box<dyn std::error::Error>> {
    if is_dir {
        std::fs::create_dir_all(outpath)?;
        return Ok(0);
    }
    if let Some(parent) = outpath.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut outfile = std::fs::File::create(outpath)?;
    std::io::copy(reader, &mut outfile)?;
    Ok(1)
}

fn unpack_zip(
    archive: &Path,
    output: &Path,
    opts: &UnpackOptions<'_>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let mut count = 0;

    std::fs::create_dir_all(output)?;

    for i in 0..zip.len() {
        let mut file = zip.by_index(i)?;
        let Some(outpath) = zip_member_outpath(file.name(), opts, output) else {
            continue;
        };
        count += write_member_to_disk(file.is_dir(), &mut file, &outpath)?;
    }

    Ok(count)
}

fn write_tar_entry_to_disk(
    entry: &mut tar::Entry<'_, Box<dyn Read>>,
    outpath: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if entry.header().entry_type().is_dir() {
        std::fs::create_dir_all(outpath)?;
        return Ok(());
    }
    if let Some(parent) = outpath.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut outfile = std::fs::File::create(outpath)?;
    std::io::copy(entry, &mut outfile)?;
    Ok(())
}

fn tar_entry_outpath(path: &Path, opts: &UnpackOptions<'_>, output: &Path) -> Option<PathBuf> {
    if !archive_entry_selected(path, opts.selected_files) {
        return None;
    }
    let rel = strip_archive_path(path, opts.strip_components)?;
    if rel.as_os_str().is_empty() {
        return None;
    }
    Some(output.join(rel))
}

fn unpack_one_tar_entry(
    entry: &mut tar::Entry<'_, Box<dyn Read>>,
    output: &Path,
    opts: &UnpackOptions<'_>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let path = entry.path()?.into_owned();
    let Some(outpath) = tar_entry_outpath(&path, opts, output) else {
        return Ok(0);
    };
    let is_file = !entry.header().entry_type().is_dir();
    write_tar_entry_to_disk(entry, &outpath)?;
    Ok(usize::from(is_file))
}

fn unpack_tar(
    archive: &Path,
    output: &Path,
    compress: Option<&str>,
    opts: &UnpackOptions<'_>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(archive)?;
    let dec = tar_decoder_from_compress(file, compress);
    let mut tar = tar::Archive::new(dec);
    std::fs::create_dir_all(output)?;
    let mut count = 0;

    for entry in tar.entries()? {
        let mut entry = entry?;
        count += unpack_one_tar_entry(&mut entry, output, opts)?;
    }

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

fn zip_list_detail(size: u64, modified: Option<impl std::fmt::Debug>) -> String {
    let modified = modified
        .map(|dt| format!("{dt:?}"))
        .unwrap_or_else(|| "unknown".to_string());
    format!("{size} bytes, {modified}")
}

fn list_zip(
    archive: &Path,
    verbose: bool,
    max_entries: usize,
) -> Result<String, Box<dyn std::error::Error>> {
    let file = std::fs::File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file)?;
    let total = zip.len();
    let header = format!("ZIP 归档: {} ({} 个文件)", archive.display(), total);
    let mut lines = Vec::new();
    let mut all_paths = Vec::new();

    for i in 0..zip.len() {
        let file = zip.by_index(i)?;
        let name = file.name().to_string();
        all_paths.push(name.clone());
        let detail = zip_list_detail(file.size(), file.last_modified());
        maybe_push_list_line(
            &mut lines,
            max_entries,
            format_archive_list_line(&name, verbose, &detail),
        );
    }

    Ok(format_capped_list(
        &header,
        total,
        max_entries,
        lines,
        &all_paths,
    ))
}

fn list_tar(
    archive: &Path,
    verbose: bool,
    max_entries: usize,
) -> Result<String, Box<dyn std::error::Error>> {
    let dec = open_tar_list_decoder(archive)?;
    let mut tar = tar::Archive::new(dec);
    let mut count = 0usize;
    let mut lines = Vec::new();
    let mut all_paths = Vec::new();

    for entry in tar.entries()? {
        let entry = entry?;
        let display = entry.path()?.display().to_string();
        all_paths.push(display.clone());
        count += 1;
        let detail = format!("{} bytes", entry.size());
        maybe_push_list_line(
            &mut lines,
            max_entries,
            format_archive_list_line(&display, verbose, &detail),
        );
    }

    let header = format!("TAR 归档: {} ({} 个文件)", archive.display(), count);
    Ok(format_capped_list(
        &header,
        count,
        max_entries,
        lines,
        &all_paths,
    ))
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

#[cfg(test)]
mod archive_tool_tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn detect_format_prefers_compound_tar_suffixes() {
        assert_eq!(detect_format("a.TAR.GZ"), "tar.gz");
        assert_eq!(detect_format("a.tgz"), "tar.gz");
        assert_eq!(detect_format("a.tar"), "tar");
        assert_eq!(detect_format("a.zip"), "zip");
        assert_eq!(detect_format("a.bin"), "unknown");
    }

    #[test]
    fn tar_magic_kind_matches_gzip_bzip_xz() {
        assert_eq!(tar_magic_kind(*b"\x1f\x8b"), "gzip");
        assert_eq!(tar_magic_kind(*b"BZ"), "bzip2");
        assert_eq!(tar_magic_kind(*b"\xfd\x37"), "xz");
        assert_eq!(tar_magic_kind(*b"\x00\x00"), "none");
    }

    #[test]
    fn strip_archive_path_removes_prefix_levels() {
        let p = Path::new("hpcg-HPCG-release-3-1-0/README.md");
        let stripped = strip_archive_path(p, 1).unwrap();
        assert_eq!(stripped, Path::new("README.md"));
    }

    #[test]
    fn capped_list_truncates_with_top_level_summary() {
        let paths: Vec<String> = (0..300).map(|i| format!("cmake-{i}/file.txt")).collect();
        let lines: Vec<String> = paths.iter().take(5).map(|p| format!("  {p}")).collect();
        let out = format_capped_list("HDR", 300, 5, lines, &paths);
        assert!(out.contains("已截断"));
        assert!(out.contains("顶层目录/文件"));
    }

    #[test]
    fn unpack_tar_honors_strip_components() {
        let dir = tempdir().unwrap();
        let archive = dir.path().join("pkg.tar");
        let inner = dir.path().join("src");
        fs::create_dir_all(&inner).unwrap();
        fs::write(inner.join("hello.txt"), b"hi").unwrap();

        {
            let file = fs::File::create(&archive).unwrap();
            let mut tar = tar::Builder::new(file);
            tar.append_dir_all("pkg-root", &inner).unwrap();
            tar.finish().unwrap();
        }

        let out = dir.path().join("out");
        let opts = UnpackOptions {
            strip_components: 1,
            selected_files: &[],
        };
        let n = unpack_tar(&archive, &out, None, &opts).unwrap();
        assert_eq!(n, 1);
        assert!(out.join("hello.txt").is_file());
    }
}
