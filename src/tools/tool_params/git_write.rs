//! 工具 JSON 参数 schema（按领域拆分；由 `tool_params` 再导出）。

pub(in crate::tools) fn params_git_stage_files() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "paths":{"type":"array","items":{"type":"string"},"description":"要暂存的相对路径列表（必填）"}
        },
        "required":["paths"]
    })
}

pub(in crate::tools) fn params_git_commit() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "message":{"type":"string","description":"提交信息（必填）"},
            "stage_all":{"type":"boolean","description":"可选：提交前是否执行 git add -A，默认 false"},
            "confirm":{"type":"boolean","description":"安全确认；仅当 true 时才会执行 commit"}
        },
        "required":["message"]
    })
}

pub(in crate::tools) fn params_git_fetch() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "remote":{"type":"string","description":"可选：远程名，如 origin"},
            "branch":{"type":"string","description":"可选：分支名（与 remote 一起使用）"},
            "prune":{"type":"boolean","description":"可选：是否 --prune，默认 false"}
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_git_remote_set_url() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "name":{"type":"string","description":"远程名（必填）"},
            "url":{"type":"string","description":"远程 URL（必填）"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":["name","url"]
    })
}

pub(in crate::tools) fn params_git_apply() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "patch_path":{"type":"string","description":"补丁文件相对路径（必填）"},
            "check_only":{"type":"boolean","description":"是否仅检查可应用性，默认 true"}
        },
        "required":["patch_path"]
    })
}

pub(in crate::tools) fn params_git_clone() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "repo_url":{"type":"string","description":"仓库 URL（必填）"},
            "target_dir":{"type":"string","description":"工作区内目标相对目录（必填）"},
            "depth":{"type":"integer","description":"可选：浅克隆深度（--depth）","minimum":1},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":["repo_url","target_dir"]
    })
}

pub(in crate::tools) fn params_git_checkout() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "target":{"type":"string","description":"分支名、标签名或 commit SHA（必填）"},
            "create":{"type":"boolean","description":"是否以 -b 创建新分支，默认 false"}
        },
        "required":["target"]
    })
}

pub(in crate::tools) fn params_git_branch_create() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "name":{"type":"string","description":"新分支名（必填）"},
            "start_point":{"type":"string","description":"可选：起始点（分支/tag/SHA），默认 HEAD"}
        },
        "required":["name"]
    })
}

pub(in crate::tools) fn params_git_branch_delete() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "name":{"type":"string","description":"要删除的分支名（必填）"},
            "force":{"type":"boolean","description":"是否强制删除（-D），默认 false（-d，需已合并）"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":["name"]
    })
}

pub(in crate::tools) fn params_git_push() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "remote":{"type":"string","description":"远程名，默认 origin"},
            "branch":{"type":"string","description":"可选：推送的分支/refspec"},
            "set_upstream":{"type":"boolean","description":"是否 -u 设置上游，默认 false"},
            "force_with_lease":{"type":"boolean","description":"是否 --force-with-lease，默认 false"},
            "tags":{"type":"boolean","description":"是否 --tags 推送标签，默认 false"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_git_merge() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "branch":{"type":"string","description":"要合并的分支名（必填）"},
            "no_ff":{"type":"boolean","description":"是否 --no-ff 强制合并提交，默认 false"},
            "squash":{"type":"boolean","description":"是否 --squash 压缩合并，默认 false"},
            "message":{"type":"string","description":"可选：合并消息"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":["branch"]
    })
}

pub(in crate::tools) fn params_git_rebase() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "onto":{"type":"string","description":"变基目标（分支/SHA）"},
            "abort":{"type":"boolean","description":"是否 --abort 取消变基"},
            "continue":{"type":"boolean","description":"是否 --continue 继续变基"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_git_stash() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "action":{"type":"string","description":"操作：push（默认）/pop/apply/list/drop/clear","enum":["push","pop","apply","list","drop","clear"]},
            "message":{"type":"string","description":"可选：push 时的描述消息"},
            "confirm":{"type":"boolean","description":"仅 clear 需要 confirm=true"}
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_git_tag() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "action":{"type":"string","description":"操作：list（默认）/create/delete","enum":["list","create","delete"]},
            "name":{"type":"string","description":"create/delete 时的标签名"},
            "message":{"type":"string","description":"create 时的注释消息（传入即创建 annotated tag）"},
            "pattern":{"type":"string","description":"list 时的 glob 过滤（如 v*）"},
            "confirm":{"type":"boolean","description":"仅 delete 需要 confirm=true"}
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_git_reset() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "mode":{"type":"string","description":"重置模式：soft/mixed（默认）/hard","enum":["soft","mixed","hard"]},
            "target":{"type":"string","description":"目标 commit/ref，默认 HEAD"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_git_cherry_pick() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "commit":{"type":"string","description":"要挑选的单个 commit SHA"},
            "commits":{"type":"array","items":{"type":"string"},"description":"要挑选的多个 commit SHA"},
            "no_commit":{"type":"boolean","description":"是否 --no-commit 仅暂存不提交，默认 false"},
            "abort":{"type":"boolean","description":"是否 --abort"},
            "continue":{"type":"boolean","description":"是否 --continue"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":[]
    })
}

pub(in crate::tools) fn params_git_revert() -> serde_json::Value {
    serde_json::json!({
        "type":"object",
        "properties":{
            "commit":{"type":"string","description":"要回滚的 commit SHA（必填，除 abort/continue）"},
            "no_commit":{"type":"boolean","description":"是否 --no-commit 仅暂存不提交，默认 false"},
            "abort":{"type":"boolean","description":"是否 --abort"},
            "continue":{"type":"boolean","description":"是否 --continue"},
            "confirm":{"type":"boolean","description":"安全确认；仅 true 才执行"}
        },
        "required":[]
    })
}

// ── Node.js / npm / npx ─────────────────────────────────────
