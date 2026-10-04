//! 寄せ環境の向き [`FlushDirection`]。

/// 寄せ環境（`flushleft` / `center` / `flushright`）が本体の段落を寄せる向き（固定 3 種）。
///
/// 構文が生む向きはこの 3 つだけなので、`[text].alignment` の 4 値のうち両端揃えを持たない。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FlushDirection {
  /// 左寄せ（`flushleft`）
  Left,
  /// 中央寄せ（`center`）
  Center,
  /// 右寄せ（`flushright`）
  Right,
}
