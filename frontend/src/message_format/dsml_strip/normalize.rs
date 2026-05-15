pub(super) fn normalize_deepseek_dsml_vendor_variants(s: &str) -> String {
    s.replace("<｜｜DSML｜｜", "<｜DSML｜")
        .replace("</｜｜DSML｜｜", "</｜DSML｜")
        .replace("<||DSML||", "<|DSML|")
        .replace("</||DSML||", "</|DSML|")
}

pub(super) fn collapse_blank_runs(s: &str) -> String {
    let lines: Vec<&str> = s.lines().map(str::trim_end).collect();
    lines
        .split(|line| line.is_empty())
        .map(|g| g.join("\n"))
        .filter(|b| !b.is_empty())
        .collect::<Vec<_>>()
        .join("\n\n")
        .trim()
        .to_string()
}
