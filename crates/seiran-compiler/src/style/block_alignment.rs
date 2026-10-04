//! 自然幅の内容を寄せる 3 値の揃え [`BlockAlignment`]。

use garde::Validate;
use serde::Deserialize;

/// 自然幅の内容（数式ブロックの本体・目次と索引の 1 行）を利用可能幅の中で寄せる向き
/// （`[math.block].alignment` / `[toc].alignment` / `[index].alignment` / `[index].title_alignment`）。
///
/// 伸縮しない 1 単位を寄せるだけなので両端揃えを持たない。`"justify"` は未知の値として拒否される。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Validate)]
#[serde(rename_all = "snake_case")]
#[garde(allow_unvalidated)]
pub(crate) enum BlockAlignment {
  /// 左揃え
  Left,
  /// 中央揃え
  Center,
  /// 右揃え
  Right,
}
