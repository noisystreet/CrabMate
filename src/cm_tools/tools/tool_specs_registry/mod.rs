//! 工具规格表：按领域拆分为 `specs/*.inc.rs`（各文件为单个数组字面量），运行时在 `OnceLock` 中拼接为 `&'static [ToolSpec]`。

use std::sync::OnceLock;

use crate::cm_names as tool_names;
use crate::cm_tools::tools::http_fetch::{HttpFetchArgs, HttpRequestArgs};
use crate::cm_tools::tools::tool_json_schema::tool_parameters_schema_value as schema_of;
use crate::cm_tools::tools::tool_param_types as args;

use super::tool_summary as ts;
use super::*;

#[cfg(feature = "archive-tools")]
static SPECS_ARCHIVE: &[ToolSpec] = &include!("specs/archive.inc.rs");
#[cfg(not(feature = "archive-tools"))]
static SPECS_ARCHIVE: &[ToolSpec] = &[];
static SPECS_BASIC_NETWORK: &[ToolSpec] = &include!("specs/basic_network.inc.rs");
static SPECS_EXEC_PACKAGE: &[ToolSpec] = &include!("specs/exec_package.inc.rs");
static SPECS_CARGO_RUST: &[ToolSpec] = &include!("specs/cargo_rust.inc.rs");
static SPECS_FRONTEND_PYTHON: &[ToolSpec] = &include!("specs/frontend_python.inc.rs");
static SPECS_DOCS_HEALTH: &[ToolSpec] = &include!("specs/docs_health.inc.rs");
static SPECS_FRONTEND_AUDIT_CI: &[ToolSpec] = &include!("specs/frontend_audit_ci.inc.rs");
static SPECS_DIAGNOSTICS_DOCS: &[ToolSpec] = &include!("specs/diagnostics_docs.inc.rs");
static SPECS_GITHUB_CLI: &[ToolSpec] = &include!("specs/github_cli.inc.rs");
static SPECS_GIT_READ: &[ToolSpec] = &include!("specs/git_read.inc.rs");
static SPECS_FILE_CORE: &[ToolSpec] = &include!("specs/file_core.inc.rs");
static SPECS_MARKDOWN_STRUCTURED: &[ToolSpec] = &include!("specs/markdown_structured.inc.rs");
static SPECS_TEXT_CODE_NAV: &[ToolSpec] = &include!("specs/text_code_nav.inc.rs");
static SPECS_SCHEDULE: &[ToolSpec] = &include!("specs/schedule.inc.rs");
static SPECS_PROCESS: &[ToolSpec] = &include!("specs/process.inc.rs");
static SPECS_METRICS: &[ToolSpec] = &include!("specs/metrics.inc.rs");
static SPECS_FILE_EXTRA: &[ToolSpec] = &include!("specs/file_extra.inc.rs");
static SPECS_MISC_BASIC: &[ToolSpec] = &include!("specs/misc_basic.inc.rs");
static SPECS_SELF_CONFIG: &[ToolSpec] = &include!("specs/self_config_info.inc.rs");

static ALL_TOOL_SPECS: OnceLock<&'static [ToolSpec]> = OnceLock::new();

pub(super) fn tool_specs() -> &'static [ToolSpec] {
    ALL_TOOL_SPECS.get_or_init(|| {
        let cap = SPECS_ARCHIVE.len()
            + SPECS_BASIC_NETWORK.len()
            + SPECS_EXEC_PACKAGE.len()
            + SPECS_CARGO_RUST.len()
            + SPECS_FRONTEND_PYTHON.len()
            + SPECS_DOCS_HEALTH.len()
            + SPECS_FRONTEND_AUDIT_CI.len()
            + SPECS_DIAGNOSTICS_DOCS.len()
            + SPECS_GITHUB_CLI.len()
            + SPECS_GIT_READ.len()
            + SPECS_FILE_CORE.len()
            + SPECS_MARKDOWN_STRUCTURED.len()
            + SPECS_TEXT_CODE_NAV.len()
            + SPECS_SCHEDULE.len()
            + SPECS_PROCESS.len()
            + SPECS_METRICS.len()
            + SPECS_FILE_EXTRA.len()
            + SPECS_MISC_BASIC.len()
            + SPECS_SELF_CONFIG.len();
        let mut v = Vec::with_capacity(cap);
        v.extend_from_slice(SPECS_ARCHIVE);
        v.extend_from_slice(SPECS_BASIC_NETWORK);
        v.extend_from_slice(SPECS_EXEC_PACKAGE);
        v.extend_from_slice(SPECS_CARGO_RUST);
        v.extend_from_slice(SPECS_FRONTEND_PYTHON);
        v.extend_from_slice(SPECS_DOCS_HEALTH);
        v.extend_from_slice(SPECS_FRONTEND_AUDIT_CI);
        v.extend_from_slice(SPECS_DIAGNOSTICS_DOCS);
        v.extend_from_slice(SPECS_GITHUB_CLI);
        v.extend_from_slice(SPECS_GIT_READ);
        v.extend_from_slice(SPECS_FILE_CORE);
        v.extend_from_slice(SPECS_MARKDOWN_STRUCTURED);
        v.extend_from_slice(SPECS_TEXT_CODE_NAV);
        v.extend_from_slice(SPECS_SCHEDULE);
        v.extend_from_slice(SPECS_PROCESS);
        v.extend_from_slice(SPECS_METRICS);
        v.extend_from_slice(SPECS_FILE_EXTRA);
        v.extend_from_slice(SPECS_MISC_BASIC);
        v.extend_from_slice(SPECS_SELF_CONFIG);
        Box::leak(v.into_boxed_slice())
    })
}

#[cfg(test)]
mod name_consistency_tests {
    use super::*;
    use crate::cm_names::ALL;
    use std::collections::HashSet;

    /// `tool_names::ALL` 与注册表必须**双向一致**。
    ///
    /// 只改 `specs` 不改 `ALL`（或反之）会让所有以 `ALL` 驱动的遍历静默漏项，
    /// 且不会有任何编译错误——这正是把工具名收敛到单一真源要防的那类缺陷。
    #[test]
    fn all_tool_names_match_specs() {
        let registered: HashSet<&str> = tool_specs().iter().map(|s| s.name).collect();
        let declared: HashSet<&str> = ALL.iter().copied().collect();

        let missing_in_all: Vec<&str> = registered.difference(&declared).copied().collect();
        let missing_in_specs: Vec<&str> = declared.difference(&registered).copied().collect();

        assert!(
            missing_in_all.is_empty(),
            "以下工具已在 specs 注册，但未在 tool_names::ALL 声明：{missing_in_all:?}"
        );
        assert!(
            missing_in_specs.is_empty(),
            "以下工具已在 tool_names::ALL 声明，但未在 specs 注册：{missing_in_specs:?}"
        );
    }

    /// 注册表内部不得有重名（重名会让 `tools_for_name` 类查找取到任意一个）。
    #[test]
    fn spec_names_are_unique() {
        let mut seen = HashSet::new();
        for spec in tool_specs() {
            assert!(seen.insert(spec.name), "specs 中工具名重复：{}", spec.name);
        }
    }

    /// 执行体种类必须钉住：`AsyncOnly` 表示「同步面没有执行体」，错标成 `Typed` 会让同步
    /// 路径出现一条本不该存在的执行分支，反之则让异步面凭空少一条。而注册表快照只比
    /// `category` / `description` / `parameters` / 摘要，**不含 runner**，这类错标它发现不了，
    /// 所以单独钉一次。
    #[test]
    fn async_only_runners_are_pinned() {
        let mut got: Vec<&str> = tool_specs()
            .iter()
            .filter(|s| matches!(s.runner, ToolRunner::AsyncOnly))
            .map(|s| s.name)
            .collect();
        got.sort_unstable();
        let mut want = vec![tool_names::RUN_COMMAND, tool_names::TERMINAL_SESSION];
        want.sort_unstable();
        assert_eq!(got, want, "AsyncOnly 工具集合发生变化");
    }
}

/// 注册流程收敛（#1157）期间的**零漂移**快照守卫。
///
/// 该 Issue 承诺「不改 schema、不改工具行为与输出」，而把 `ToolSpec` 的
/// name / description / summary 从手写改成宏派生时，最可能悄悄坏掉的正是
/// 「schema 少了个字段」与「摘要少了一段」——两者都没有编译错误，只在真实
/// 模型调用或前端展示时才暴露。因此这里把对模型与前端可见的形状钉成快照：
/// 每项的 `category` / `description` / `parameters`、摘要类型、注册顺序，以及
/// 若干固定入参下的摘要**文本**。迁移的每一批都必须让它绿着通过。
///
/// **feature 兼容**：`archive-tools` 会改变注册表（多 3 个 `archive_*`），而 CI 用
/// `cargo test --all-features` 跑测试。因此快照存**全集**，比对时只对「当前构建
/// 实际注册的工具」逐项校验、顺序按存在项取子序列，BLESS 时也保留仅在其它
/// feature 组合下出现的项——两套 feature 组合不会互相擦除。
///
/// 有意变更（增删工具、改描述、改 schema）时再生成（先 `--all-features` 存全集）：
/// `CRABMATE_BLESS=1 cargo test --all-features --lib cm_tools::tools::tool_specs_registry::registry_golden_tests::registry_golden_matches_specs`
#[cfg(test)]
mod registry_golden_tests {
    use super::*;
    use crate::cm_names as tool_names;
    use serde_json::{Map, Value, json};
    use std::collections::{BTreeMap, HashSet};
    use std::path::{Path, PathBuf};

    /// 手写入参样本：覆盖真实场景下的关键组合（`read_file` 的两种行号形态、写副作用工具、
    /// 命令类与目录类）。工具名一律引用 `cm_names` 常量——`scripts/tool-name-literal-ratchet.sh`
    /// 禁止在比较位置出现工具名字面量。
    const SUMMARY_SAMPLES: &[(&str, &str)] = &[
        (
            tool_names::READ_FILE,
            r#"{"path":"src/lib.rs","anchor_line":10,"context_lines":20}"#,
        ),
        (
            tool_names::READ_FILE,
            r#"{"path":"src/lib.rs","start_line":5,"end_line":9}"#,
        ),
        (
            tool_names::CREATE_FILE,
            r#"{"path":"notes/a.md","content":""}"#,
        ),
        (
            tool_names::MODIFY_FILE,
            r#"{"path":"a.rs","search":"x","replace":"y"}"#,
        ),
        (
            tool_names::SEARCH_IN_FILES,
            r#"{"pattern":"ToolSpec","file_glob":"*.rs"}"#,
        ),
        (
            tool_names::RUN_COMMAND,
            r#"{"command":"cargo","args":["check"]}"#,
        ),
        (tool_names::LIST_TREE, r#"{"path":"src","max_depth":2}"#),
        // `structured` 的摘要有 4 个分支（validate / query / diff / patch），
        // 按 schema 生成的单条样本只能命中其中一个，所以逐个分支各钉一条。
        (
            tool_names::STRUCTURED,
            r#"{"action":"validate","path":"Cargo.toml"}"#,
        ),
        (
            tool_names::STRUCTURED,
            r#"{"action":"query","path":"Cargo.toml","query":"/package/name"}"#,
        ),
        (
            tool_names::STRUCTURED,
            r#"{"action":"diff","path_a":"a.json","path_b":"b.json"}"#,
        ),
        (
            tool_names::STRUCTURED,
            r#"{"action":"patch","path":"a.json","query":"/x","mutation":"remove"}"#,
        ),
    ];

    fn golden_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/tool_registry_golden.json")
    }

    fn summary_shape(spec: &ToolSpec) -> Value {
        match spec.summary {
            ToolSummaryKind::None => json!("none"),
            ToolSummaryKind::Static(s) => json!({ "static": s }),
            ToolSummaryKind::Dynamic(_) => json!("dynamic"),
        }
    }

    fn dynamic_summary(name: &str, args: &Value) -> Option<String> {
        let spec = tool_specs().iter().find(|s| s.name == name)?;
        match spec.summary {
            ToolSummaryKind::Dynamic(f) => f(args),
            ToolSummaryKind::None | ToolSummaryKind::Static(_) => None,
        }
    }

    /// 当前构建真实的注册表：`(顺序, 名字 → 形状)`。
    fn current_snapshot() -> (Vec<String>, BTreeMap<String, Value>) {
        let mut order = Vec::with_capacity(tool_specs().len());
        let mut tools: BTreeMap<String, Value> = BTreeMap::new();
        for spec in tool_specs() {
            order.push(spec.name.to_string());
            tools.insert(
                spec.name.to_string(),
                json!({
                    "category": spec.category.as_str(),
                    "description": spec.description,
                    "parameters": (spec.parameters)(),
                    "summary": summary_shape(spec),
                    "tags": spec.tags,
                }),
            );
        }
        (order, tools)
    }

    /// 采样来源 ①：手写入参，挑的是真实场景下的关键组合（`read_file` 的两种行号形态、
    /// 写副作用工具、命令类与目录类），人工可读、便于对照旧摘要结构的字段语义。
    /// 采样来源 ② 见下方「按 schema 生成」的兜底。
    fn current_samples() -> Vec<Value> {
        let registered: HashSet<&str> = tool_specs().iter().map(|s| s.name).collect();
        let mut samples = Vec::new();
        let mut covered: HashSet<&str> = HashSet::new();
        for &(name, raw_args) in SUMMARY_SAMPLES {
            if !registered.contains(name) {
                continue;
            }
            let args: Value = serde_json::from_str(raw_args)
                .unwrap_or_else(|e| panic!("摘要样本 {name} 的 args 不是合法 JSON: {e}"));
            covered.insert(name);
            samples.push(json!({
                "tool": name,
                "args": args,
                "summary": dynamic_summary(name, &args),
            }));
        }
        // 采样来源 ②：手写样本没覆盖到的动态摘要工具，按 schema 生成「全字段填充」入参。
        //
        // 迁移的实质是把旧的 `*SummaryArgs` 字段子集结构换成 `impl ToolArgsSummary for XArgs`，
        // 最容易犯的错是「漏读某个字段 / 字段名写错」，而这类错只在那个字段非空时才显形。
        // 因此这里把 schema 里**所有**属性都填上占位值（不只 `required`），覆盖面比手挑几条
        // 真实入参更宽；schema 一旦改动，摘要文本随之变化、快照就会红——那正是想要的提示
        // （schema 是契约，改它必须走 BLESS 评审）。附带效果：新加的动态摘要工具即使没人补
        // 手写样本，也会被自动兜住，不会出现「摘要代码改了却没有快照守着」的工具。
        for spec in tool_specs() {
            if !matches!(spec.summary, ToolSummaryKind::Dynamic(_)) || covered.contains(spec.name) {
                continue;
            }
            let schema = (spec.parameters)();
            let args = placeholder_object(&schema, &schema, 0);
            samples.push(json!({
                "tool": spec.name,
                "args": args,
                "summary": dynamic_summary(spec.name, &args),
            }));
        }
        samples
    }

    /// 按 JSON Schema 造一份「所有属性都填上占位值」的样本入参。
    ///
    /// `root` 用于解析 `$ref`（`rust_analyzer` 这类带动作选择器的 Args 把分支放在 `$defs`），
    /// `depth` 兜住自引用 schema，避免递归不收敛。
    fn placeholder_object(root: &Value, schema: &Value, depth: u8) -> Value {
        let mut args = Map::new();
        if depth >= 3 {
            return Value::Object(args);
        }
        if let Some(props) = schema.get("properties").and_then(Value::as_object) {
            for (key, prop) in props {
                args.insert(key.clone(), placeholder_value(root, prop, depth + 1));
            }
        }
        Value::Object(args)
    }

    /// 单个属性的占位值：`$ref` → `oneOf`/`anyOf` → `type` 依次收敛。
    fn placeholder_value(root: &Value, schema: &Value, depth: u8) -> Value {
        if let Some(reference) = schema.get("$ref").and_then(Value::as_str) {
            let def = reference
                .strip_prefix("#/$defs/")
                .and_then(|name| root.get("$defs").and_then(|defs| defs.get(name)));
            return match def {
                Some(def) if depth < 3 => placeholder_value(root, def, depth + 1),
                _ => Value::Null,
            };
        }
        for key in ["oneOf", "anyOf"] {
            let Some(branches) = schema.get(key).and_then(Value::as_array) else {
                continue;
            };
            let picked = branches
                .iter()
                .find(|b| b.get("type").and_then(Value::as_str) != Some("null"))
                .or_else(|| branches.first());
            if let Some(picked) = picked {
                return placeholder_value(root, picked, depth + 1);
            }
        }
        match schema.get("type") {
            Some(Value::String(ty)) => placeholder_of_type(root, schema, ty, depth),
            // `"type": ["integer", "null"]` 这类可空类型：取第一个非 null 的标量。
            Some(Value::Array(types)) => {
                let Some(ty) = types
                    .iter()
                    .filter_map(Value::as_str)
                    .find(|ty| *ty != "null")
                else {
                    return Value::Null;
                };
                let mut narrowed = schema.clone();
                narrowed["type"] = json!(ty);
                placeholder_value(root, &narrowed, depth + 1)
            }
            _ => Value::Null,
        }
    }

    fn placeholder_of_type(root: &Value, schema: &Value, ty: &str, depth: u8) -> Value {
        match ty {
            "string" => json!("x"),
            "integer" => json!(1),
            "number" => json!(1.0),
            "boolean" => json!(false),
            "array" => match schema.get("items") {
                Some(items) => json!([placeholder_value(root, items, depth + 1)]),
                None => json!(["x"]),
            },
            "object" => placeholder_object(root, schema, depth),
            _ => Value::Null,
        }
    }

    fn read_golden(path: &Path) -> Option<Value> {
        let raw = std::fs::read_to_string(path).ok()?;
        Some(serde_json::from_str(&raw).expect("快照文件必须是合法 JSON"))
    }

    /// 以旧顺序为骨架：删掉当前已不注册的名字，再把新增名字插到其前驱之后。
    fn merged_order(previous: &[String], current: &[String]) -> Vec<String> {
        let present: HashSet<&str> = current.iter().map(String::as_str).collect();
        let mut order: Vec<String> = previous
            .iter()
            .filter(|n| present.contains(n.as_str()))
            .cloned()
            .collect();
        for (idx, name) in current.iter().enumerate() {
            if order.iter().any(|n| n == name) {
                continue;
            }
            let anchor = idx
                .checked_sub(1)
                .and_then(|prev| order.iter().position(|n| n == &current[prev]))
                .map_or(0, |pos| pos + 1);
            order.insert(anchor, name.clone());
        }
        order
    }

    /// BLESS：以当前注册表为准，但保留只在其它 feature 组合下注册的项与顺序。
    fn blessed(
        previous: Option<&Value>,
        order: &[String],
        tools: &BTreeMap<String, Value>,
        samples: &[Value],
    ) -> Value {
        let previous_order: Vec<String> = previous
            .and_then(|v| v.get("registry_order"))
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        let mut merged: BTreeMap<String, Value> = BTreeMap::new();
        if let Some(prev) = previous.and_then(|v| v.get("tools")).and_then(Value::as_object) {
            for (name, shape) in prev {
                if !tools.contains_key(name) {
                    merged.insert(name.clone(), shape.clone());
                }
            }
        }
        for (name, shape) in tools {
            merged.insert(name.clone(), shape.clone());
        }

        json!({
            "registry_order": merged_order(&previous_order, order),
            "tools": merged,
            "dynamic_summary_samples": samples,
        })
    }

    /// 顺序校验：快照顺序删掉「当前构建没注册」的项后，必须与当前顺序一致。
    fn assert_order(expected: &Value, current: &[String]) {
        let expected_order: Vec<String> = expected
            .get("registry_order")
            .and_then(Value::as_array)
            .expect("快照缺少 registry_order 数组")
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect();
        let present: HashSet<&str> = current.iter().map(String::as_str).collect();
        let filtered: Vec<&str> = expected_order
            .iter()
            .map(String::as_str)
            .filter(|n| present.contains(n))
            .collect();
        let actual: Vec<&str> = current.iter().map(String::as_str).collect();
        assert_eq!(
            filtered, actual,
            "工具注册顺序漂移（影响 /tools 与 build_tools 的列表顺序）"
        );
    }

    fn assert_tools(expected: &Value, tools: &BTreeMap<String, Value>) {
        let expected_tools: &Map<String, Value> = expected
            .get("tools")
            .and_then(Value::as_object)
            .expect("快照缺少 tools 对象");
        for (name, shape) in tools {
            let got = expected_tools.get(name).unwrap_or_else(|| {
                panic!("快照里没有工具 `{name}`；新增/改名工具后请按上文命令 BLESS")
            });
            assert_eq!(
                got, shape,
                "工具 `{name}` 的注册形状（category/description/parameters/summary）漂移"
            );
        }
    }

    fn assert_samples(expected: &Value, samples: &[Value]) {
        let expected_samples = expected
            .get("dynamic_summary_samples")
            .and_then(Value::as_array)
            .expect("快照缺少 dynamic_summary_samples 数组");
        for sample in samples {
            assert!(
                expected_samples.contains(sample),
                "摘要文本漂移，样本: {sample}"
            );
        }
    }

    #[test]
    fn registry_golden_matches_specs() {
        let (order, tools) = current_snapshot();
        let samples = current_samples();
        let path = golden_path();
        let previous = read_golden(&path);

        if std::env::var_os("CRABMATE_BLESS").is_some() {
            let merged = blessed(previous.as_ref(), &order, &tools, &samples);
            let pretty =
                serde_json::to_string_pretty(&merged).expect("序列化 tool registry 快照失败");
            std::fs::write(&path, pretty + "\n").expect("写入 fixtures/tool_registry_golden.json");
            eprintln!("已写入 {}", path.display());
            return;
        }

        let expected = previous.unwrap_or_else(|| {
            panic!(
                "读不到 {}；用 `CRABMATE_BLESS=1 cargo test --all-features --lib cm_tools::tools::tool_specs_registry::registry_golden_tests::registry_golden_matches_specs` 生成",
                path.display()
            )
        });
        assert_order(&expected, &order);
        assert_tools(&expected, &tools);
        assert_samples(&expected, &samples);
    }
}
