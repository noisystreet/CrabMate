//! `GET /workspace/git` 的 HTTP 冒烟：四态可区分、脏摘要随改动变化、缓存与 `refresh`。
//!
//! 骨架照 `handlers_profile_http_tests.rs`：`start_test_serve_with_config` + 临时目录。
//! 细粒度解析（前缀裁剪、字段偏移、异常形状）由 `git_projection` 的单测覆盖，这里只验接线。

use std::path::Path;

use crate::test_serve::start_test_serve_with_config;

fn loopback_http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .no_proxy()
        .build()
        .expect("reqwest client")
}

fn cfg_default() -> crate::config::AgentConfig {
    crate::config::load_config(None).expect("embed default")
}

/// 在 `dir` 内跑 git（带本地身份，避免依赖宿主机 git 配置）。
fn git(dir: &Path, args: &[&str]) {
    let mut cmd = std::process::Command::new("git");
    cmd.arg("-c")
        .arg("user.name=crabmate-test")
        .arg("-c")
        .arg("user.email=test@example.com");
    cmd.args(args).current_dir(dir);
    let out = cmd.output().expect("spawn git");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn init_repo(dir: &Path) {
    git(dir, &["init", "-q"]);
    git(dir, &["commit", "-q", "--allow-empty", "-m", "init"]);
}

/// `POST /workspace` 设根，返回 `GET /workspace/git` 的响应体。
async fn setup_and_get(
    client: &reqwest::Client,
    base: &str,
    workspace: &Path,
    query: &str,
) -> serde_json::Value {
    let set = client
        .post(format!("{base}/workspace"))
        .json(&serde_json::json!({ "path": workspace }))
        .send()
        .await
        .expect("POST /workspace");
    assert!(set.status().is_success(), "set workspace: {:?}", set);
    let resp = client
        .get(format!("{base}/workspace/git{query}"))
        .send()
        .await
        .expect("GET /workspace/git");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    resp.json().await.expect("git json body")
}

#[tokio::test]
async fn clean_repo_reports_branch_and_not_dirty() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    let body = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    assert_eq!(body["state"], "ok", "{body}");
    assert_eq!(body["head"]["kind"], "branch", "{body}");
    assert_eq!(body["dirty"], false, "{body}");
    assert_eq!(body["source"], "probe", "{body}");
    assert!(body.get("reason_code").is_none(), "{body}");
}

#[tokio::test]
async fn new_file_makes_worktree_dirty() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    std::fs::write(ws.path().join("a.txt"), "x").expect("write");
    let body = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    assert_eq!(body["dirty"], true, "{body}");
    assert_eq!(body["counts"]["untracked"], 1, "{body}");
    assert_eq!(body["counts"]["total"], 1, "{body}");
}

#[tokio::test]
async fn detached_head_is_distinguishable() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    git(ws.path(), &["checkout", "-q", "--detach"]);
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    let body = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    assert_eq!(body["state"], "ok", "{body}");
    assert_eq!(body["head"]["kind"], "detached", "{body}");
    assert!(
        body["head"].get("name").is_none(),
        "detached 不应出现 name 键：{body}"
    );
}

#[tokio::test]
async fn non_repo_directory_reports_not_a_repo() {
    let ws = tempfile::tempdir().expect("tempdir");
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    let body = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    assert_eq!(body["state"], "not_a_repo", "{body}");
    assert!(body.get("head").is_none(), "{body}");
    assert!(body.get("counts").is_none(), "{body}");
}

#[tokio::test]
async fn unset_workspace_reports_failed_workspace_not_set() {
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    let resp = client
        .get(format!("{}/workspace/git", handle.base_url))
        .send()
        .await
        .expect("GET /workspace/git");
    assert_eq!(resp.status(), reqwest::StatusCode::OK);
    let body: serde_json::Value = resp.json().await.expect("body");
    assert_eq!(body["state"], "failed", "{body}");
    assert_eq!(body["reason_code"], "WORKSPACE_NOT_SET", "{body}");
}

#[tokio::test]
async fn second_request_within_ttl_is_served_from_cache() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    let first = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    assert_eq!(first["source"], "probe", "{first}");
    let second = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    assert_eq!(second["source"], "cache", "{second}");
    assert_eq!(
        second["computed_at_unix"], first["computed_at_unix"],
        "命中缓存不应推进探测时刻：{second}"
    );
}

#[tokio::test]
async fn refresh_bypasses_cache() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    let _ = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    let body = setup_and_get(&client, &handle.base_url, ws.path(), "?refresh=true").await;
    assert_eq!(body["source"], "probe", "{body}");
}

#[tokio::test]
async fn records_outside_workspace_subdir_are_not_counted() {
    let repo = tempfile::tempdir().expect("tempdir");
    init_repo(repo.path());
    let sub = repo.path().join("sub");
    std::fs::create_dir_all(&sub).expect("mkdir sub");
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    // 工作区根 = 仓库子目录；改动落在根外。
    std::fs::write(repo.path().join("outside.txt"), "x").expect("write outside");
    let body = setup_and_get(&client, &handle.base_url, &sub, "").await;
    assert_eq!(body["state"], "ok", "{body}");
    assert_eq!(body["dirty"], false, "根外改动不应计入：{body}");

    // 根内改动才计入。
    std::fs::write(sub.join("inside.txt"), "x").expect("write inside");
    let body = setup_and_get(&client, &handle.base_url, &sub, "?refresh=true").await;
    assert_eq!(body["dirty"], true, "{body}");
    assert_eq!(body["counts"]["untracked"], 1, "{body}");
}

/// `?files=true`：三条改动各一条 entry，标记与 `git status` 一致（issue 验收第 1 条）。
#[tokio::test]
async fn files_true_lists_staged_unstaged_and_untracked() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    std::fs::write(ws.path().join("tracked.txt"), "a").expect("write");
    git(ws.path(), &["add", "tracked.txt"]);
    git(ws.path(), &["commit", "-q", "-m", "add tracked"]);

    std::fs::write(ws.path().join("staged.txt"), "s").expect("write staged");
    git(ws.path(), &["add", "staged.txt"]);
    std::fs::write(ws.path().join("tracked.txt"), "b").expect("write tracked");
    std::fs::write(ws.path().join("untracked.txt"), "u").expect("write untracked");

    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();
    let body = setup_and_get(&client, &handle.base_url, ws.path(), "?files=true").await;

    assert_eq!(body["counts"]["total"], 3, "{body}");
    assert_eq!(body["files"]["truncated"], false, "{body}");
    assert_eq!(body["files"]["limit"], 500, "{body}");
    assert_eq!(body["untracked_mode"], "folded", "{body}");
    let items = body["files"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 3, "计数与列表必须同口径：{body}");

    let find = |p: &str| {
        items
            .iter()
            .find(|e| e["path"] == p)
            .unwrap_or_else(|| panic!("缺少 {p}: {body}"))
    };
    let staged = find("staged.txt");
    assert_eq!(staged["kind"], "file");
    assert_eq!(staged["staged"], true);
    assert_eq!(staged["unstaged"], false);
    let unstaged = find("tracked.txt");
    assert_eq!(unstaged["staged"], false);
    assert_eq!(unstaged["unstaged"], true);
    let untracked = find("untracked.txt");
    assert_eq!(untracked["untracked"], true);
    assert_eq!(untracked["staged"], false);
}

/// 无参请求不返回列表：侧栏高频轮询不该搬整份清单。
#[tokio::test]
async fn files_absent_by_default() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    std::fs::write(ws.path().join("a.txt"), "x").expect("write");
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    let body = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    assert!(body.get("files").is_none(), "默认不带列表：{body}");
    assert_eq!(body["untracked_mode"], "folded", "{body}");
    assert_eq!(body["counts"]["total"], 1, "{body}");
}

/// #1498 缓存回归：先无参请求（写缓存）→ 再 `?files=true` 必须拿到列表。
#[tokio::test]
async fn files_served_after_plain_request_hit_cache() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    std::fs::write(ws.path().join("a.txt"), "x").expect("write");
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    let plain = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    assert_eq!(plain["source"], "probe", "{plain}");

    let body = setup_and_get(&client, &handle.base_url, ws.path(), "?files=true").await;
    assert_eq!(body["source"], "cache", "{body}");
    let items = body["files"]["items"]
        .as_array()
        .expect("缓存命中仍须带列表，不得被前一次无参请求污染");
    assert_eq!(items.len(), 1, "{body}");
    assert_eq!(items[0]["path"], "a.txt", "{body}");
}

/// 折叠的未跟踪目录 ⇒ 一条 `kind=dir`，`counts.untracked` 仍是 1（口径不变）。
#[tokio::test]
async fn collapsed_untracked_dir_listed_as_dir() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    std::fs::create_dir_all(ws.path().join("newdir")).expect("mkdir");
    std::fs::write(ws.path().join("newdir/f1"), "1").expect("write f1");
    std::fs::write(ws.path().join("newdir/f2"), "2").expect("write f2");
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    let body = setup_and_get(&client, &handle.base_url, ws.path(), "?files=true").await;
    assert_eq!(body["counts"]["untracked"], 1, "折叠口径：{body}");
    let items = body["files"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "{body}");
    assert_eq!(items[0]["path"], "newdir", "{body}");
    assert_eq!(items[0]["kind"], "dir", "{body}");
}

/// 宿主 `core.quotePath=true`（git 默认值）时，非 ASCII 路径必须原样返回。
///
/// 没钉死 `core.quotePath=false` 或丢了 `-z` 时，这里会拿到 `"\344\270\255…"` 这类转义。
#[tokio::test]
async fn path_kept_verbatim_under_default_quotepath() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    git(ws.path(), &["config", "core.quotePath", "true"]);
    std::fs::write(ws.path().join("中文 名.txt"), "x").expect("write");
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    let body = setup_and_get(&client, &handle.base_url, ws.path(), "?files=true").await;
    let items = body["files"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "{body}");
    assert_eq!(items[0]["path"], "中文 名.txt", "{body}");
}

/// #1498 bug 回归：仓库设 `status.relativePaths=false` 时，根外记录不得混进来。
///
/// 该配置让 `git status` 输出**仓库根相对**路径：根外记录不再以 `../` 开头，
/// 旧实现的裁剪判据直接失效（计数偏大；出列表后就是根外路径外泄）。
#[tokio::test]
async fn relative_paths_false_does_not_leak_outside_records() {
    let repo = tempfile::tempdir().expect("tempdir");
    init_repo(repo.path());
    git(repo.path(), &["config", "status.relativePaths", "false"]);
    let sub = repo.path().join("sub");
    std::fs::create_dir_all(&sub).expect("mkdir sub");
    std::fs::write(repo.path().join("outside.txt"), "x").expect("write outside");

    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();
    let body = setup_and_get(&client, &handle.base_url, &sub, "?files=true").await;

    assert_eq!(body["state"], "ok", "{body}");
    assert_eq!(body["counts"]["total"], 0, "根外改动不得计入：{body}");
    assert_eq!(body["dirty"], false, "{body}");
    let items = body["files"]["items"].as_array().expect("items");
    assert!(items.is_empty(), "根外路径不得入列：{body}");
    assert!(
        !items.iter().any(|e| e["path"] == "outside.txt"),
        "根外路径泄露：{body}"
    );
}

/// 仓库设 `status.renames=false` 时，`git mv` 会被拆成一删一增；
/// 钉死 `status.renames=true` 后仍是一条记录（否则 `counts.total` 翻倍）。
#[tokio::test]
async fn renames_pinned_true_keeps_one_record() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    std::fs::write(ws.path().join("a.txt"), "a").expect("write");
    git(ws.path(), &["add", "a.txt"]);
    git(ws.path(), &["commit", "-q", "-m", "add a"]);
    git(ws.path(), &["config", "status.renames", "false"]);
    git(ws.path(), &["mv", "a.txt", "b.txt"]);

    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();
    let body = setup_and_get(&client, &handle.base_url, ws.path(), "?files=true").await;

    assert_eq!(body["counts"]["total"], 1, "rename 只算一条：{body}");
    let items = body["files"]["items"].as_array().expect("items");
    assert_eq!(items.len(), 1, "{body}");
    assert_eq!(items[0]["path"], "b.txt", "{body}");
}

/// `refresh=true` 与 `files=true` 组合：强制重探后仍要带列表。
///
/// `force` 会跳过缓存读、失败时仍可能落到 `stale_cache`，所以 `want_files` 必须一路穿透。
#[tokio::test]
async fn refresh_with_files_still_returns_list() {
    let ws = tempfile::tempdir().expect("tempdir");
    init_repo(ws.path());
    std::fs::write(ws.path().join("a.txt"), "x").expect("write");
    let handle = start_test_serve_with_config(cfg_default(), None).await;
    let client = loopback_http_client();

    // 先无参写缓存，再 `refresh=true&files=true`：必须真探且带列表。
    let _ = setup_and_get(&client, &handle.base_url, ws.path(), "").await;
    let body = setup_and_get(
        &client,
        &handle.base_url,
        ws.path(),
        "?refresh=true&files=true",
    )
    .await;
    assert_eq!(body["source"], "probe", "{body}");
    let items = body["files"]["items"]
        .as_array()
        .expect("refresh 之后仍须带列表");
    assert_eq!(items.len(), 1, "{body}");
    assert_eq!(items[0]["path"], "a.txt", "{body}");
}
