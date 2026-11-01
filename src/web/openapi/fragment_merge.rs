//! OpenAPI 片段合并（`paths` / `components` 两处共用）。
//!
//! #1239：`openapi_paths::merge_path_fragments` 与 `openapi_components::merge_component_objects`
//! 除 panic 文案外字节级相同，合并为一个函数；`prefix` 只参与报错文本，调用点传入各自的
//! 原始文案，使失败信息与合并前**逐字一致**（`/openapi.json` 输出不涉及该路径，故契约零变化）。

use serde_json::{Map, Value};

/// 把多个 `Value::Object` 片段合并为单个 object；重复 key 或非 object 片段即 panic。
///
/// 两个标签原样出现在 panic 文案里：调用点各传入自己**原有**的文本
/// （原本两处的大小写并不统一——`openapi path` 与 `duplicate OpenAPI path key`），
/// 因此失败信息与合并前逐字一致。
pub(super) fn merge_object_fragments(
    fragment_label: &str,
    duplicate_label: &str,
    fragments: &[Value],
) -> Value {
    let mut map = Map::new();
    for fragment in fragments {
        let Value::Object(o) = fragment else {
            panic!("{fragment_label} fragment must be a JSON object");
        };
        for (k, v) in o {
            if map.insert(k.clone(), v.clone()).is_some() {
                panic!("duplicate {duplicate_label} key: {k}");
            }
        }
    }
    Value::Object(map)
}
