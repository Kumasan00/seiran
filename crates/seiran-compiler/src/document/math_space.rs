//! 数式の明示アキの種類 [`MathSpace`]。

/// 数式の明示アキの種類（`\thinspace` 等）
///
/// 量ではなく種類を持つ。細・中・太は自動スペーシングの細アキ・中アキ・太アキと同じものを指し、量は typeset が自動アキと
/// 同じ定数から決める。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MathSpace {
  /// 細アキ（`\thinspace`）
  Thin,
  /// 中アキ（`\medspace`）
  Medium,
  /// 太アキ（`\thickspace`）
  Thick,
  /// 1em（`\quad`）
  Quad,
  /// 負の細アキ（`\negthinspace`）
  NegativeThin,
}
