//! 和文約物（括弧・句読点・中点）のクラス分類と JIS X 4051 の前後アキ規則
//!
//! 句点類と読点類を分け、句点後のアキは収縮能力を持たない（JLReq §3.8.3 の追込みに句点後は入らない）。

use crate::typeset::boxes::GlueKind;

/// 二分（1em の 1/2）
const NIBU: f32 = 0.5;

/// 四分（1em の 1/4）
const SHIBU: f32 = 0.25;

/// 和文約物のクラス（JIS X 4051 の約物処理単位）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum YakumonoClass {
  /// 始め括弧類（前アキを内蔵。例: `（「『【〔`）
  Open,
  /// 終わり括弧類（後アキを内蔵。例: `）」』】〕`）
  Close,
  /// 読点類（後アキを内蔵。例: `、，`）
  Comma,
  /// 句点類（後アキを内蔵し、行中のアキは追込みに使わない。例: `。．`）
  FullStop,
  /// 中点類（両側にアキを内蔵。例: `・：；`）
  MiddleDot,
  /// 約物以外（漢字・仮名・英数字など。全角ベタ組の対象）
  Normal,
}

/// 約物グリフをアキ抜きの実寸ボックスへ正規化する量（1em に対する倍率）
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Normalize {
  /// 送り幅から差し引くアキの合計（em）
  pub(super) trim_em: f32,
  /// 墨を左へ寄せる量（em）。左側にアキを持つ約物（始め括弧・中点）で正となる
  pub(super) shift_em: f32,
}

/// 約物境界に挿入するアキ（1em に対する倍率）
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Aki {
  /// 標準アキ（em）。左揃え（`left`）でもこの幅で並ぶ
  pub(super) natural_em: f32,
  /// 詰め可能量（em）。両端揃えの行で収縮に使える量で、句点後のアキは 0
  pub(super) shrink_em: f32,
  /// アキを決めた規則（中点の規則か括弧・句読点の規則か）
  pub(super) kind: GlueKind,
}

/// 文字を約物クラスへ分類する
pub(super) fn classify(ch: char) -> YakumonoClass {
  return match ch {
    '（' | '「' | '『' | '【' | '〔' | '〈' | '《' | '［' | '｛' | '〖' | '〘' | '〚' | '｟' => {
      YakumonoClass::Open
    },
    '）' | '」' | '』' | '】' | '〕' | '〉' | '》' | '］' | '｝' | '〗' | '〙' | '〛' | '｠' => {
      YakumonoClass::Close
    },
    '、' | '，' => YakumonoClass::Comma,
    '。' | '．' => YakumonoClass::FullStop,
    '・' | '：' | '；' => YakumonoClass::MiddleDot,
    _ => YakumonoClass::Normal,
  };
}

/// 約物グリフの正規化量を返す（`Normal` は正規化しないので `None`）
pub(super) fn normalize(class: YakumonoClass) -> Option<Normalize> {
  return match class {
    YakumonoClass::Close | YakumonoClass::Comma | YakumonoClass::FullStop => Some(Normalize {
      trim_em: NIBU,
      shift_em: 0.0,
    }),
    YakumonoClass::Open => Some(Normalize {
      trim_em: NIBU,
      shift_em: NIBU,
    }),
    YakumonoClass::MiddleDot => Some(Normalize {
      trim_em: SHIBU * 2.0,
      shift_em: SHIBU,
    }),
    YakumonoClass::Normal => None,
  };
}

/// 隣接する約物クラス対（`left` → `right`）の境界アキを返す
pub(super) fn gap(left: YakumonoClass, right: YakumonoClass) -> Option<Aki> {
  use YakumonoClass::{Close, Comma, FullStop, MiddleDot, Normal, Open};

  // 中点は片側四分。中点が絡む境界を先に処理する
  if left == MiddleDot || right == MiddleDot {
    // 括弧内側の禁則ベタ（始め括弧直後・終わり括弧/句読点直前）は中点でもアキなし
    if left == Open || matches!(right, Close | Comma | FullStop) {
      return None;
    }
    return Some(aki(left, SHIBU, GlueKind::MiddleDotAki));
  }

  return match (left, right) {
    // 前アキ二分（何か → 始め括弧）／後アキ二分（終わり括弧・句読点 → 通常文字）
    (Normal | Close | Comma | FullStop, Open) | (Close | Comma | FullStop, Normal) => {
      Some(aki(left, NIBU, GlueKind::PunctuationAki))
    },
    // それ以外（括弧内側・連続約物・通常文字どうし）はベタ
    _ => None,
  };
}

/// 左が `left` の境界の、規則 `kind` が決めた標準アキ `em` の [`Aki`] を作る（句点後を除き、約物アキは全量ベタまで
/// 詰められる）
fn aki(left: YakumonoClass, em: f32, kind: GlueKind) -> Aki {
  return Aki {
    kind,
    natural_em: em,
    shrink_em: if left == YakumonoClass::FullStop {
      0.0
    } else {
      em
    },
  };
}

#[cfg(test)]
mod tests {
  use super::{Aki, GlueKind, NIBU, Normalize, SHIBU, YakumonoClass, classify, gap, normalize};

  #[test]
  fn classify_sorts_each_class() {
    assert_eq!(classify('（'), YakumonoClass::Open);
    assert_eq!(classify('「'), YakumonoClass::Open);
    assert_eq!(classify('）'), YakumonoClass::Close);
    assert_eq!(classify('」'), YakumonoClass::Close);
    assert_eq!(classify('、'), YakumonoClass::Comma);
    assert_eq!(classify('。'), YakumonoClass::FullStop);
    assert_eq!(classify('・'), YakumonoClass::MiddleDot);
    assert_eq!(classify('あ'), YakumonoClass::Normal);
    assert_eq!(classify('漢'), YakumonoClass::Normal);
  }

  #[test]
  fn classify_separates_full_stops_from_commas() {
    assert_eq!(classify('。'), YakumonoClass::FullStop);
    assert_eq!(classify('．'), YakumonoClass::FullStop);
    assert_eq!(classify('、'), YakumonoClass::Comma);
    assert_eq!(classify('，'), YakumonoClass::Comma);
  }

  #[test]
  fn gap_protects_the_aki_after_a_full_stop() {
    let protected = |em, kind| {
      return Aki {
        natural_em: em,
        shrink_em: 0.0,
        kind,
      };
    };

    assert_eq!(
      gap(YakumonoClass::FullStop, YakumonoClass::Normal),
      Some(protected(NIBU, GlueKind::PunctuationAki)),
      "。kanji"
    );
    assert_eq!(
      gap(YakumonoClass::FullStop, YakumonoClass::Open),
      Some(protected(NIBU, GlueKind::PunctuationAki)),
      "。「"
    );
    assert_eq!(
      gap(YakumonoClass::FullStop, YakumonoClass::MiddleDot),
      Some(protected(SHIBU, GlueKind::MiddleDotAki)),
      "。・"
    );
    assert_eq!(
      gap(YakumonoClass::Comma, YakumonoClass::Normal),
      Some(aki(NIBU, GlueKind::PunctuationAki)),
      "、kanji は詰められる"
    );
    assert_eq!(
      gap(YakumonoClass::Close, YakumonoClass::Normal),
      Some(aki(NIBU, GlueKind::PunctuationAki)),
      "。」の後の 」kanji は終わり括弧後"
    );
  }

  #[test]
  fn gap_inserts_nibu_before_open_and_after_close_comma() {
    assert_eq!(
      gap(YakumonoClass::Normal, YakumonoClass::Open),
      Some(aki(NIBU, GlueKind::PunctuationAki)),
      "kanji「"
    );
    assert_eq!(gap(YakumonoClass::Close, YakumonoClass::Open), Some(aki(NIBU, GlueKind::PunctuationAki)), "」「");
    assert_eq!(gap(YakumonoClass::Comma, YakumonoClass::Open), Some(aki(NIBU, GlueKind::PunctuationAki)), "、「");
    assert_eq!(
      gap(YakumonoClass::Close, YakumonoClass::Normal),
      Some(aki(NIBU, GlueKind::PunctuationAki)),
      "」kanji"
    );
    assert_eq!(
      gap(YakumonoClass::Comma, YakumonoClass::Normal),
      Some(aki(NIBU, GlueKind::PunctuationAki)),
      "、kanji"
    );
  }

  #[test]
  fn gap_collapses_consecutive_and_inside_brackets() {
    assert_eq!(gap(YakumonoClass::Comma, YakumonoClass::Close), None, "、」");
    assert_eq!(gap(YakumonoClass::Close, YakumonoClass::Comma), None, "」、");
    assert_eq!(gap(YakumonoClass::FullStop, YakumonoClass::Close), None, "。」");
    assert_eq!(gap(YakumonoClass::Close, YakumonoClass::FullStop), None, "」。");
    assert_eq!(gap(YakumonoClass::MiddleDot, YakumonoClass::FullStop), None, "・。");
    assert_eq!(gap(YakumonoClass::Close, YakumonoClass::Close), None, "」」");
    assert_eq!(gap(YakumonoClass::Open, YakumonoClass::Normal), None, "「kanji");
    assert_eq!(gap(YakumonoClass::Normal, YakumonoClass::Close), None, "kanji」");
    assert_eq!(gap(YakumonoClass::Open, YakumonoClass::Open), None, "「「");
  }

  #[test]
  fn gap_uses_shibu_around_middle_dot() {
    assert_eq!(
      gap(YakumonoClass::Normal, YakumonoClass::MiddleDot),
      Some(aki(SHIBU, GlueKind::MiddleDotAki)),
      "kanji・"
    );
    assert_eq!(
      gap(YakumonoClass::MiddleDot, YakumonoClass::Normal),
      Some(aki(SHIBU, GlueKind::MiddleDotAki)),
      "・kanji"
    );
    assert_eq!(gap(YakumonoClass::Open, YakumonoClass::MiddleDot), None, "「・");
    assert_eq!(gap(YakumonoClass::MiddleDot, YakumonoClass::Close), None, "・」");
  }

  #[test]
  fn normalize_trims_and_shifts_per_class() {
    assert_eq!(
      normalize(YakumonoClass::Close),
      Some(Normalize {
        trim_em: NIBU,
        shift_em: 0.0
      }),
      "終わり括弧は右アキを抜くだけ"
    );
    assert_eq!(
      normalize(YakumonoClass::Comma),
      Some(Normalize {
        trim_em: NIBU,
        shift_em: 0.0
      }),
      "句読点は右アキを抜くだけ"
    );
    assert_eq!(
      normalize(YakumonoClass::Open),
      Some(Normalize {
        trim_em: NIBU,
        shift_em: NIBU
      }),
      "始め括弧は左アキを抜き墨を二分左へ"
    );
    assert_eq!(
      normalize(YakumonoClass::MiddleDot),
      Some(Normalize {
        trim_em: SHIBU * 2.0,
        shift_em: SHIBU
      }),
      "中点は両側四分を抜き墨を四分左へ"
    );
    assert_eq!(
      normalize(YakumonoClass::FullStop),
      Some(Normalize {
        trim_em: NIBU,
        shift_em: 0.0
      }),
      "句点は読点と同じく右アキを抜くだけ"
    );
    assert_eq!(normalize(YakumonoClass::Normal), None, "通常文字は正規化しない");
  }

  /// テスト用: 規則 `kind` の詰められる標準アキ（標準アキ = 詰め代）
  fn aki(em: f32, kind: GlueKind) -> Aki {
    return Aki {
      natural_em: em,
      shrink_em: em,
      kind,
    };
  }
}
