//! Bandit 扫描：`bandit_scan` 命令行装配。

use std::path::Path;
use std::process::Command;

use super::{MAX_PATHS, filter_existing, parse_rel_paths_from_slice, run_and_format};
use crate::tools::tool_param_types::{
    BanditConfidenceArg, BanditOutputFormat, BanditScanArgs, BanditSeverityArg,
};

pub fn bandit_scan(args_json: &str, workspace_root: &Path, max_output_len: usize) -> String {
    let v = match crate::tools::parse_args_json(args_json) {
        Ok(v) => v,
        Err(e) => return e,
    };
    let args: BanditScanArgs = match serde_json::from_value(v) {
        Ok(a) => a,
        Err(e) => return format!("参数 JSON 与 bandit_scan 形状不一致: {e}"),
    };
    let base = match workspace_root.canonicalize() {
        Ok(p) => p,
        Err(e) => return format!("工作区根目录无法解析: {e}"),
    };
    let paths = match parse_rel_paths_from_slice(&args.paths, "paths", &["."], MAX_PATHS) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let paths = filter_existing(&base, &paths);

    let mut cmd = Command::new("bandit");
    cmd.arg("-r").current_dir(&base);

    if let Some(sev) = args.severity {
        match sev {
            BanditSeverityArg::Low => {
                cmd.arg("-ll");
            }
            BanditSeverityArg::Medium => {
                cmd.arg("-ll");
            }
            BanditSeverityArg::High => {
                cmd.arg("-lll");
            }
        }
    }

    if let Some(conf) = args.confidence {
        match conf {
            BanditConfidenceArg::Low => {
                cmd.arg("-i");
            }
            BanditConfidenceArg::Medium => {
                cmd.arg("-ii");
            }
            BanditConfidenceArg::High => {
                cmd.arg("-iii");
            }
        }
    }

    if let Some(skip) = args
        .skip
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        if skip.len() > 512 || skip.contains('\n') || skip.contains("..") {
            return "错误：skip 值过长或含非法字符".to_string();
        }
        cmd.arg("--skip").arg(skip);
    }

    if let Some(fmt) = args.format {
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

    for p in &paths {
        cmd.arg(p);
    }
    run_and_format(cmd, max_output_len, "bandit")
}
