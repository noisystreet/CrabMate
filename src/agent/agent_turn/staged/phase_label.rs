//! 分阶段 FSM 枚举的 `as_str()` 宏生成器。
//!
//! 消除 12+ 处手写 `match self { Variant => "label", ... }` 的重复模式。
//! 用法：
//! ```ignore
//! impl_as_str!(MyEnum, {
//!     Self::VariantA => "variant_a",
//!     Self::VariantB { .. } => "variant_b",
//! });
//! ```

/// 生成 `pub(crate) fn as_str(&self) -> &'static str`。
///
/// 支持单元变体和带数据的变体（用 `{ .. }` 匹配）。
#[macro_export]
macro_rules! impl_as_str {
    ($enum:ident, { $($variant:pat => $label:expr),+ $(,)? }) => {
        impl $enum {
            pub(crate) fn as_str(&self) -> &'static str {
                match self {
                    $( $variant => $label, )+
                }
            }
        }
    };
}
