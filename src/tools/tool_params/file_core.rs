//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_file_write() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "相对工作目录的文件路径，如 subdir/name.txt"
            },
            "content": {
                "type": "string",
                "description": "要写入的文件内容"
            }
        },
        "required": ["path"]
    })
}

pub(in crate::tools) fn params_modify_file() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "相对工作目录的文件路径"
            },
            "mode": {
                "type": "string",
                "description": "可选：full（默认）整文件覆盖；replace_lines 按行区间替换，流式读写适合大文件",
                "enum": ["full", "replace_lines"]
            },
            "content": {
                "type": "string",
                "description": "full 时为新的全文；replace_lines 时替换区间的新内容（可为空以删除这些行）"
            },
            "start_line": {
                "type": "integer",
                "description": "replace_lines 必填：起始行（1-based，含）",
                "minimum": 1
            },
            "end_line": {
                "type": "integer",
                "description": "replace_lines 必填：结束行（1-based，含）",
                "minimum": 1
            }
        },
        "required": ["path"]
    })
}

pub(in crate::tools) fn params_file_from_to_overwrite() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "from": {
                "type": "string",
                "description": "源文件路径（相对工作目录）"
            },
            "to": {
                "type": "string",
                "description": "目标文件路径（相对工作目录）；父目录不存在时会创建"
            },
            "overwrite": {
                "type": "boolean",
                "description": "目标已存在且为文件时是否覆盖；默认 false"
            }
        },
        "required": ["from", "to"],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_read_file() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "相对工作目录的文件路径，如 src/main.rs"
            },
            "start_line": {
                "type": "integer",
                "description": "可选：起始行（1-based，包含该行）；省略时默认为 1",
                "minimum": 1
            },
            "end_line": {
                "type": "integer",
                "description": "可选：结束行（1-based，包含该行）；省略时按 max_lines 自动截断到 start_line+max_lines-1 或 EOF",
                "minimum": 1
            },
            "max_lines": {
                "type": "integer",
                "description": "单次最多返回行数，默认 500，上限 8000；防止大文件一次读爆上下文",
                "minimum": 1,
                "maximum": 8000
            },
            "count_total_lines": {
                "type": "boolean",
                "description": "可选：是否额外扫描全文件统计总行数（大文件会多一次 I/O，默认 false）"
            },
            "encoding": {
                "type": "string",
                "description": "可选：文本编码。默认 utf-8（严格，非法 UTF-8 会报错）；另有 utf-8-sig（去 BOM）、gb18030、gbk、gb2312、big5、utf-16le、utf-16be、auto（BOM 优先，否则嗅探）。非法序列不静默替换。"
            }
        },
        "required": ["path"]
    })
}

pub(in crate::tools) fn params_glob_files() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "glob 模式（相对起始目录），如 **/*.rs、*.toml、src/**/*.ts；禁止 .. 与绝对路径"
            },
            "path": {
                "type": "string",
                "description": "可选：起始子目录（相对工作区，默认 .）"
            },
            "max_depth": {
                "type": "integer",
                "description": "可选：相对起始目录最大路径层数（默认 20，上限 100）；0 表示仅起始目录下的一层，不进入子目录",
                "minimum": 0,
                "maximum": 100
            },
            "max_results": {
                "type": "integer",
                "description": "可选：最多返回多少条匹配路径（默认 200，上限 5000）",
                "minimum": 1,
                "maximum": 5000
            },
            "include_hidden": {
                "type": "boolean",
                "description": "可选：是否进入/匹配以 . 开头的文件与目录，默认 false"
            }
        },
        "required": ["pattern"],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_list_tree() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": {
                "type": "string",
                "description": "可选：起始目录（相对工作区，默认 .）"
            },
            "max_depth": {
                "type": "integer",
                "description": "可选：相对起始目录最大路径层数（默认 8，上限 60）；控制向下递归层数",
                "minimum": 0,
                "maximum": 60
            },
            "max_entries": {
                "type": "integer",
                "description": "可选：最多列出多少条路径（含起点 .，默认 500，上限 10000）",
                "minimum": 1,
                "maximum": 10000
            },
            "include_hidden": {
                "type": "boolean",
                "description": "可选：是否列出以 . 开头的项，默认 false"
            }
        },
        "required": [],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_file_exists() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path": { "type":"string", "description":"相对工作目录的路径（文件或目录，必填）" },
            "kind": { "type":"string", "description":"可选：匹配类型 file|dir|any，默认 any", "enum":["file","dir","any"] }
        },
        "required":["path"],
        "additionalProperties":false
    })
}

pub(in crate::tools) fn params_read_binary_meta() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "相对工作区的文件路径（必填）" },
            "prefix_hash_bytes": {
                "type": "integer",
                "description": "参与 SHA256 的文件头字节数：默认 8192；0 表示不计算哈希；最大 262144",
                "minimum": 0,
                "maximum": 262144
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_hash_file() -> serde_json::Value {
    let max_prefix: i64 = (4u64 * 1024 * 1024 * 1024).min(i64::MAX as u64) as i64;
    serde_json::json!({
        "type": "object",
        "properties": {
            "path": { "type": "string", "description": "相对工作区的文件路径（必填）" },
            "algorithm": {
                "type": "string",
                "description": "哈希算法：sha256（默认）、sha512、blake3",
                "enum": ["sha256", "sha-256", "sha512", "sha-512", "blake3"]
            },
            "max_bytes": {
                "type": "integer",
                "description": "可选：仅哈希文件前若干字节（与整文件校验不同）；省略则整文件流式哈希。最小 1，上限 4GiB",
                "minimum": 1,
                "maximum": max_prefix
            }
        },
        "required": ["path"],
        "additionalProperties": false
    })
}

pub(in crate::tools) fn params_extract_in_file() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path": { "type":"string", "description":"相对工作目录的文件路径（必填）" },
            "pattern": { "type":"string", "description":"要匹配的正则表达式（必填）" },
            "start_line": { "type":"integer", "description":"可选：起始行（1-based，包含该行）", "minimum":1 },
            "end_line": { "type":"integer", "description":"可选：结束行（1-based，包含该行）", "minimum":1 },
            "max_matches": { "type":"integer", "description":"可选：最多返回匹配条数（默认 50）", "minimum":1 },
            "case_insensitive": { "type":"boolean", "description":"可选：是否忽略大小写（默认 true）" },
            "max_snippet_chars": { "type":"integer", "description":"可选：每条匹配行最多截断字符数（默认 400）", "minimum":1 },
            "mode": { "type":"string", "description":"可选：提取模式（lines 或 rust_fn_block，默认 lines）", "enum":["lines","rust_fn_block"] },
            "max_block_chars": { "type":"integer", "description":"可选：rust_fn_block 模式下每个块最多截断字符数（默认 8000）", "minimum":1 },
            "max_block_lines": { "type":"integer", "description":"可选：rust_fn_block 模式下每个块最多扫描/输出的行数（默认 500）", "minimum":1 },
            "encoding": {
                "type": "string",
                "description": "可选：与 read_file 相同（utf-8 / utf-8-sig / gb18030 / gbk / gb2312 / big5 / utf-16le / utf-16be / auto）；默认 utf-8 严格解码"
            }
        },
        "required":["path","pattern"],
        "additionalProperties":false
    })
}

pub(in crate::tools) fn params_apply_patch() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "patch": {
                "type": "string",
                "description": "unified diff（同 git diff）：含 ---/+++ 与 @@；变更上下各保留 2～3 行上下文（空格行），禁止零上下文；小步单主题便于回滚。路径：要么 --- src/x.rs（strip 默认 0），要么 --- a/src/x.rs 且 strip=1。可 patch -R 或 git checkout 撤销。"
            },
            "strip": {
                "type": "integer",
                "description": "patch -p：无 a/ 前缀时用 0；--- a/... 的 Git 风格 diff 须用 1",
                "minimum": 0
            }
        },
        "required": ["patch"]
    })
}

// params_search_in_files: superseded by params_search_in_files_enhanced

pub(in crate::tools) fn params_codebase_semantic_search() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "query": {
                "type": "string",
                "description": "自然语言查询；与 rebuild_index=true 二选一（重建时可省略）"
            },
            "rebuild_index": {
                "type": "boolean",
                "description": "为 true 时重建向量索引并写入 .crabmate/。整库默认增量（见 incremental / codebase_semantic_rebuild_incremental）；指定 path 时为该子树全量替换。首次或需强制对齐时可用 incremental:false"
            },
            "incremental": {
                "type": "boolean",
                "description": "仅 rebuild_index 时有效：整库是否按文件指纹跳过未改文件（默认取配置 codebase_semantic_rebuild_incremental）；false 时清空并全量重嵌入"
            },
            "path": {
                "type": "string",
                "description": "可选：相对工作区的子目录，仅索引或缩小扫描范围"
            },
            "top_k": {
                "type": "integer",
                "description": "返回最相近的块数量，默认取配置 codebase_semantic_top_k，范围 1～64",
                "minimum": 1,
                "maximum": 64
            },
            "query_max_chunks": {
                "type": "integer",
                "description": "本次 query 最多扫描多少个向量块，默认取配置 codebase_semantic_query_max_chunks；1～2000000，0 表示不限制（大索引慎用，结果为近似 Top-K）",
                "minimum": 0,
                "maximum": 2000000
            },
            "file_glob": {
                "type": "string",
                "description": "可选：仅处理文件名匹配此 glob 的文件（如 *.rs）"
            },
            "extensions": {
                "type": "array",
                "items": { "type": "string" },
                "description": "可选：覆盖默认源码扩展名列表（不含点，如 rs、ts）；省略时使用内置常见代码/文档扩展名"
            },
            "retrieve_mode": {
                "type": "string",
                "description": "检索模式（仅 query 时）：`hybrid`（默认）= SQLite FTS5 BM25 + 向量余弦加权；`semantic_only` 仅向量；`fts_only` 仅全文（需关键词能分词命中）。重建索引后 FTS 与块表由触发器同步。"
            },
            "hybrid_alpha": {
                "type": "number",
                "description": "hybrid 时向量权重 α∈[0,1]，综合分 = α×cosine + (1-α)×fts_norm；默认取配置 codebase_semantic_hybrid_alpha"
            },
            "fts_top_n": {
                "type": "integer",
                "description": "hybrid / fts_only 时 FTS 分支最多取多少条（BM25）；默认 codebase_semantic_fts_top_n，1～10000",
                "minimum": 1,
                "maximum": 10000
            },
            "hybrid_semantic_pool": {
                "type": "integer",
                "description": "hybrid 时向量扫描保留的候选块数（≥ top_k），再与 FTS 并集重排；默认 codebase_semantic_hybrid_semantic_pool，1～10000",
                "minimum": 1,
                "maximum": 10000
            }
        }
    })
}

pub(in crate::tools) fn params_search_in_files_enhanced() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "pattern": {
                "type": "string",
                "description": "要搜索的正则或纯文本关键字（使用 Rust 正则语法）"
            },
            "path": {
                "type": "string",
                "description": "可选的子目录或文件相对路径，仅在该路径下搜索（相对于工作区根目录）"
            },
            "max_results": {
                "type": "integer",
                "description": "最多返回多少条匹配结果，默认 200 条",
                "minimum": 1
            },
            "case_insensitive": {
                "type": "boolean",
                "description": "是否大小写不敏感匹配，默认 true"
            },
            "ignore_hidden": {
                "type": "boolean",
                "description": "是否忽略隐藏文件和目录（以点开头），默认 true"
            },
            "context_before": {
                "type": "integer",
                "description": "可选：每个匹配前显示的上下文行数（0-10），默认 0",
                "minimum": 0,
                "maximum": 10
            },
            "context_after": {
                "type": "integer",
                "description": "可选：每个匹配后显示的上下文行数（0-10），默认 0",
                "minimum": 0,
                "maximum": 10
            },
            "file_glob": {
                "type": "string",
                "description": "可选：仅搜索文件名匹配此 glob 模式的文件（如 *.rs、*.py）"
            },
            "exclude_glob": {
                "type": "string",
                "description": "可选：排除文件名匹配此 glob 模式的文件（如 *.min.js）"
            }
        },
        "required": ["pattern"]
    })
}

pub(in crate::tools) fn params_read_dir_enhanced() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "path": { "type":"string", "description":"可选：相对工作目录的目录路径（默认 .）" },
            "max_entries": { "type":"integer", "description":"可选：最多返回多少条目录项（默认 200）", "minimum":1 },
            "include_hidden": { "type":"boolean", "description":"可选：是否包含隐藏文件/目录（以 . 开头），默认 false" },
            "include_size": { "type":"boolean", "description":"可选：是否显示文件大小，默认 false" },
            "include_mtime": { "type":"boolean", "description":"可选：是否显示修改时间，默认 false" },
            "sort_by": { "type":"string", "description":"排序方式：name（默认）/size/mtime", "enum":["name","size","mtime"] }
        },
        "required":[]
    })
}

// ── 新增纯内存 / 开发辅助工具参数 ────────────────────────────
