//! Bandit 扫描：`bandit_scan` 命令行装配。

use std::path::Path;
use std::process::Command;

use super::{MAX_PATHS, filter_existing, parse_rel_paths_from_slice, run_and_format};
use crate::tools::tool_param_types::{
    BanditConfidenceArg, BanditOutputFormat, BanditScanArgs, BanditSeverityArg,
};

fn bandit_push_severity(cmd: &mut Command, sev: BanditSeverityArg) {
    let flag = match sev {
        BanditSeverityArg::Low | BanditSeverityArg::Medium => "-ll",
        BanditSeverityArg::High => "-lll",
    };
    cmd.arg(flag);
}

fn bandit_push_confidence(cmd: &mut Command, conf: BanditConfidenceArg) {
    let flag = match conf {
        BanditConfidenceArg::Low => "-i",
        BanditConfidenceArg::Medium => "-ii",
        BanditConfidenceArg::High => "-iii",
    };
    cmd.arg(flag);
}

fn bandit_push_output_format(cmd: &mut Command, fmt: BanditOutputFormat) {
    let s = match fmt {
        BanditOutputFormat::Txt => "txt",
        BanditOutputFormat::Json => "json",
        BanditOutputFormat::Csv => "csv",
        BanditOutputFormat::Xml => "xml",
        BanditOutputFormat::Html => "html",
        BanditOutputFormat::Yaml => "yaml",
        BanditOutputFormat::Screen => "screen",
        BanditOutputFormat::Custom => "custom",
    };
    cmd.arg("-f").arg(s);
}

fn bandit_validate_skip(skip: &str) -> Result<(), &'static str> {
    if skip.len() > 512 || skip.contains('\n') || skip.contains("..") {
        return Err("错误：skip 值过长或含非法字符");
    }
    Ok(())
}

struct BanditScanPrepared {
    base: std::path::PathBuf,
    paths: Vec<String>,
    args: BanditScanArgs,
}

fn prepare_bandit_scan(
    args_json: &str,
    workspace_root: &Path,
) -> Result<BanditScanPrepared, String> {
    let v = crate::tools::parse_args_json(args_json)?;
    let args: BanditScanArgs = serde_json::from_value(v)
        .map_err(|e| format!("参数 JSON 与 bandit_scan 形状不一致: {e}"))?;
    let base = workspace_root
        .canonicalize()
        .map_err(|e| format!("工作区根目录无法解析: {e}"))?;
    let paths = parse_rel_paths_from_slice(&args.paths, "paths", &["."], MAX_PATHS)?;
    let paths = filter_existing(&base, &paths);
    Ok(BanditScanPrepared { base, paths, args })
}

pub fn bandit_scan(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let BanditScanPrepared { base, paths, args } =
        match prepare_bandit_scan(args_json, workspace_root) {
            Ok(p) => p,
            Err(e) => return e,
        };

    let mut cmd = Command::new("bandit");
    cmd.arg("-r").current_dir(&base);

    if let Some(sev) = args.severity {
        bandit_push_severity(&mut cmd, sev);
    }

    if let Some(conf) = args.confidence {
        bandit_push_confidence(&mut cmd, conf);
    }

    if let Some(skip) = args
        .skip
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if let Err(msg) = bandit_validate_skip(skip) {
            return msg.to_string();
        }
        cmd.arg("--skip").arg(skip);
    }

    if let Some(fmt) = args.format {
        bandit_push_output_format(&mut cmd, fmt);
    }

    for p in &paths {
        cmd.arg(p);
    }
    run_and_format(cmd, max_output_len, "bandit")
}
