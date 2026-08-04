//! 脚注エリアへの詰め込み計算（#227）— 純粋関数・データのみ。`PageComposer` には依存しない。

use tracing::warn;

use crate::{model::Length, typeset::layout::Line};

/// `demands` を全行そのまま積んだときの脚注エリアの高さ（pt、固定費込み）を返す（純粋関数）
pub(super) fn footnote_area_full(demands: &[FootnoteDemand], reserved: Length, charges: FootnoteCharges) -> Length {
  let mut area = reserved;
  for demand in demands {
    area += charges.entry_overhead(area) + demand.full_height();
  }
  return area;
}

/// 行 1 個の脚注を、リージョンの脚注エリアへどう収めるかの判定結果
pub(super) enum LineFootnoteFit {
  /// 全部そのまま入る。値はエリアの新しい高さ（pt）
  Full(Length),
  /// 入り切らないので分割する。エリアの新しい高さ（pt）と脚注ごとの配置行数
  Split(Length, Vec<usize>),
  /// この行はこのリージョンに置けない（脚注の先頭 1 行すら入らない、または行自体が入らない）
  Rejected,
}

/// 行 `i` をベースライン基準で置いたとき、その行の脚注をリージョンの脚注エリアへどう収めるかを
/// 決める（純粋関数）
pub(super) fn fit_line_footnotes(
  demands: &[FootnoteDemand],
  reserved: Length,
  body_bottom: Length,
  page_limit: Length,
  charges: FootnoteCharges,
) -> LineFootnoteFit {
  let full = footnote_area_full(demands, reserved, charges);
  if body_bottom <= page_limit - full {
    return LineFootnoteFit::Full(full);
  }
  if demands.is_empty() {
    // 脚注が無いのに収まらない = 行自体がリージョンに入らない（分割する余地は無い）
    return LineFootnoteFit::Rejected;
  }
  let budget = page_limit - body_bottom - reserved;
  return match pack_footnotes(demands, reserved, budget, charges, true) {
    Some(packing) => LineFootnoteFit::Split(reserved + packing.height, packing.splits),
    None => LineFootnoteFit::Rejected,
  };
}

/// 脚注エリアの高さ課金パラメータ（`style.footnote` 由来、`geom` から切り出した純粋な値）
#[derive(Debug, Clone, Copy)]
pub(super) struct FootnoteCharges {
  /// 本文と区切り罫線の間隔（`style.footnote.top_margin`）
  pub(super) top_margin: Length,
  /// 区切り罫線の太さ（0 のとき描画しない、`style.footnote.rule_thickness`）
  pub(super) rule_thickness: Length,
  /// 区切り罫線〜最初の脚注、および脚注どうしの間隔（`style.footnote.rule_gap`）
  pub(super) rule_gap: Length,
}

impl FootnoteCharges {
  /// ページジオメトリから課金パラメータを取り出す
  pub(super) fn of(geom: &super::PageGeometry) -> Self {
    return FootnoteCharges {
      top_margin: geom.footnote_top_margin,
      rule_thickness: geom.footnote_rule_thickness,
      rule_gap: geom.footnote_rule_gap,
    };
  }

  /// 脚注エリアを `base_reserved` まで確保済みのリージョンへ、脚注 1 個を新たに置くときの固定費（pt）
  fn entry_overhead(self, base_reserved: Length) -> Length {
    if base_reserved == Length::ZERO {
      return self.top_margin + self.rule_thickness + self.rule_gap;
    }
    return self.rule_gap;
  }
}

/// 脚注 1 個の分割可能な需要（純粋データ）
pub(super) struct FootnoteDemand {
  /// 先頭 `k` 行の積み上げ高さ（長さ = 行数 + 1）
  prefix: Vec<Length>,
}

impl FootnoteDemand {
  /// 行分割済みの脚注本体から需要を組み立てる
  pub(super) fn new(lines: &[Line], leading: Length) -> Self {
    let mut prefix = Vec::with_capacity(lines.len() + 1);
    prefix.push(Length::ZERO);
    let mut baseline = Length::ZERO;
    let mut prev_depth = Length::ZERO;
    for (i, line) in lines.iter().enumerate() {
      if i == 0 {
        baseline = line.height;
      } else {
        baseline += leading.max(prev_depth + line.height);
      }
      prev_depth = line.depth;
      prefix.push(baseline + prev_depth);
    }
    return FootnoteDemand { prefix };
  }

  /// 本体の行数
  pub(super) fn line_count(&self) -> usize { return self.prefix.len() - 1; }

  /// 全行を置くのに要する本体高さ（pt）
  fn full_height(&self) -> Length { return *self.prefix.last().expect("prefix は必ず prefix[0] を持つ"); }

  /// 先頭 1 行だけを置くのに要する本体高さ（pt）。行が無ければ 0
  fn first_line_height(&self) -> Length { return self.prefix.get(1).copied().unwrap_or(Length::ZERO); }

  /// 高さ `allowance` に収まる最大の行数を返す（`prefix` の単調増加性を使う）
  fn fit_lines(&self, allowance: Length) -> usize {
    let mut fit = 0;
    for (k, height) in self.prefix.iter().enumerate().skip(1) {
      if *height > allowance {
        break;
      }
      fit = k;
    }
    return fit;
  }
}

/// 脚注エリアへの詰め込み結果
pub(super) struct FootnotePacking {
  /// 脚注ごとの、このリージョンへ置く行数（入力 `demands` と同順・同長）。
  /// 行数未満なら残りは繰り越す
  pub(super) splits: Vec<usize>,
  /// この詰め込みで脚注エリアに追加される高さ（pt、固定費込み）
  pub(super) height: Length,
}

/// 脚注エリアの「予算に対して何行入るか」を決める唯一の純粋関数（#227）
pub(super) fn pack_footnotes(
  demands: &[FootnoteDemand],
  base_reserved: Length,
  budget: Length,
  charges: FootnoteCharges,
  require_first_line: bool,
) -> Option<FootnotePacking> {
  let mut splits = Vec::with_capacity(demands.len());
  let mut height = Length::ZERO;
  for (j, demand) in demands.iter().enumerate() {
    let overhead = charges.entry_overhead(base_reserved + height);
    // 後続の脚注に最低 1 行ずつ残す（`require_first_line` のときのみ）。この脚注を置いた後なので
    // 後続の固定費は `rule_gap` だけになる
    let rest_min: Length = if require_first_line {
      demands[j + 1..].iter().map(|rest| return charges.rule_gap + rest.first_line_height()).sum()
    } else {
      Length::ZERO
    };
    let mut placed = demand.fit_lines(budget - height - overhead - rest_min);
    if placed == 0 && demand.line_count() > 0 {
      if require_first_line {
        // この行の脚注は先頭 1 行すら置けない。呼び出し側が行ごと次リージョンへ送る
        return None;
      }
      if j > 0 {
        // 繰越: 手前の脚注で予算が尽きた。以降はこのリージョンに置かない（出現順を保つ）
        splits.extend(std::iter::repeat_n(0, demands.len() - j));
        return Some(FootnotePacking { splits, height });
      }
      // 繰越の先頭が 1 行も入らない病的ケース。次リージョンへ送っても改善しないので、
      // オーバーフローを許容して 1 行進める（[`super::PageComposer::seed_carry`] のループの停止条件）
      warn!("脚注 1 行の高さがページ全体を超えるため、オーバーフローしたまま配置します");
      placed = 1;
    }
    height += overhead + demand.prefix[placed];
    splits.push(placed);
    // 分割された脚注より後ろをこのリージョンに置くと繰越と出現順が入れ替わる。`require_first_line` の
    // ときは後続の最低 1 行を予約済みなので、そのまま詰め続けてよい
    if placed < demand.line_count() && !require_first_line {
      splits.extend(std::iter::repeat_n(0, demands.len() - j - 1));
      return Some(FootnotePacking { splits, height });
    }
  }
  return Some(FootnotePacking { splits, height });
}

/// 脚注を「先頭 `placed` 行」と「残り」に分ける
pub(super) fn split_pending(
  mut footnote: super::PendingFootnote,
  placed: usize,
) -> (Option<super::PendingFootnote>, Option<super::PendingFootnote>) {
  if placed >= footnote.lines.len() {
    return (Some(footnote), None);
  }
  let rest = footnote.lines.split_off(placed);
  let tail = super::PendingFootnote {
    number: footnote.number,
    index: footnote.index,
    continued: true,
    lines: rest,
    leading: footnote.leading,
  };
  if placed == 0 {
    return (None, Some(tail));
  }
  return (Some(footnote), Some(tail));
}
