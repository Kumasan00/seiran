//! 和文約物（括弧・句読点・中点）のクラス分類と JIS X 4051 の前後アキ規則
//!
//! 標準の全角約物グリフは片側（始め括弧は左、終わり括弧・句読点は右、中点は両側）に
//! 固有のアキ（二分・四分）を送り幅として内蔵する。本モジュールは約物を 4 クラスに分類し、
//! グリフの内蔵アキを抜いた実寸へ正規化する量（[`normalize`]）と、隣接クラス対ごとの
//! 標準アキ・詰め可能量（[`gap`]）を与える。呼び出し側（`super::split_japanese_run`）は
//! これを `HItem::Glue` として組み、連続約物の重複アキ詰め・行頭行末の版面揃え・両端揃えの
//! 収縮点化を実現する。
//!
//! 内蔵アキ量は「全角 1em ＝ 二分の墨 ＋ 二分のアキ」という慣習値（pTeX の min10 / jis JFM と
//! 同前提）を用いる。実測ではなく規約値なので、半角約物を積む非標準フォントは呼び出し側の
//! 全角判定ガードで正規化対象外にする。

/// 二分（1em の 1/2）
pub(crate) const NIBU: f32 = 0.5;

/// 四分（1em の 1/4）
pub(crate) const SHIBU: f32 = 0.25;

/// 和文約物のクラス（JIS X 4051 の約物処理単位）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum YakumonoClass {
  /// 始め括弧類（前アキを内蔵。例: `（「『【〔`）
  Open,
  /// 終わり括弧類（後アキを内蔵。例: `）」』】〕`）
  Close,
  /// 句読点類（後アキを内蔵。例: `、。，．`）
  Comma,
  /// 中点類（両側にアキを内蔵。例: `・：；`）
  MiddleDot,
  /// 約物以外（漢字・仮名・英数字など。全角ベタ組の対象）
  Normal,
}

/// 約物グリフをアキ抜きの実寸ボックスへ正規化する量（1em に対する倍率）
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Normalize {
  /// 送り幅から差し引くアキの合計（em）
  pub(crate) trim_em: f32,
  /// 墨を左へ寄せる量（em）。左側にアキを持つ約物（始め括弧・中点）で正となる
  pub(crate) shift_em: f32,
}

/// 約物境界に挿入するアキ（1em に対する倍率）
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Aki {
  /// 標準アキ（em）。`ragged_right` でもこの幅で並ぶ
  pub(crate) natural_em: f32,
  /// 詰め可能量（em）。両端揃えの収縮点として字間より先に吸収される
  pub(crate) shrink_em: f32,
}

/// 文字を約物クラスへ分類する
///
/// 未収載の文字（漢字・仮名・英数字・全角スペース等）はすべて [`YakumonoClass::Normal`]。
pub(crate) fn classify(ch: char) -> YakumonoClass {
  return match ch {
    // 始め括弧類（全角・互換全角）
    '（' | '「' | '『' | '【' | '〔' | '〈' | '《' | '［' | '｛' | '〖' | '〘' | '〚' | '｟' => {
      YakumonoClass::Open
    },
    // 終わり括弧類
    '）' | '」' | '』' | '】' | '〕' | '〉' | '》' | '］' | '｝' | '〗' | '〙' | '〛' | '｠' => {
      YakumonoClass::Close
    },
    // 句読点類
    '、' | '。' | '，' | '．' => YakumonoClass::Comma,
    // 中点類
    '・' | '：' | '；' => YakumonoClass::MiddleDot,
    _ => YakumonoClass::Normal,
  };
}

/// 約物グリフの正規化量を返す（`Normal` は正規化しないので `None`）
///
/// - 終わり括弧・句読点: 右にアキ → 幅を二分詰め、墨は動かさない
/// - 始め括弧: 左にアキ → 幅を二分詰め、墨を二分左へ寄せる
/// - 中点: 両側にアキ → 幅を四分×2 詰め、墨を四分左へ寄せる
pub(crate) fn normalize(class: YakumonoClass) -> Option<Normalize> {
  return match class {
    YakumonoClass::Close | YakumonoClass::Comma => Some(Normalize {
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
///
/// `None` は「アキなし（ベタ）」を表す。連続約物の重複アキ（`。」`・`」。`・`」」` 等）や
/// 括弧内側（`「…` 直後・`…」` 直前）はここで `None` となり、余分なアキが残らない。
/// `Normal → Normal` は本表の対象外（字間ベタ + 分割点の伸長は呼び出し側の既存経路が扱う）で
/// `None` を返す。境界の分割可否（行頭行末の版面揃えに使う breakable）は ICU の分割可能位置で
/// 別途決めるため、本表は幅だけを与える。
pub(crate) fn gap(left: YakumonoClass, right: YakumonoClass) -> Option<Aki> {
  use YakumonoClass::{Close, Comma, MiddleDot, Normal, Open};

  // 中点は片側四分。中点が絡む境界を先に処理する
  if left == MiddleDot || right == MiddleDot {
    // 括弧内側の禁則ベタ（始め括弧直後・終わり括弧/句読点直前）は中点でもアキなし
    if left == Open || right == Close || right == Comma {
      return None;
    }
    return Some(aki(SHIBU));
  }

  return match (left, right) {
    // 前アキ二分（何か → 始め括弧）／後アキ二分（終わり括弧・句読点 → 通常文字）
    (Normal | Close | Comma, Open) | (Close | Comma, Normal) => Some(aki(NIBU)),
    // それ以外（括弧内側・連続約物・通常文字どうし）はベタ
    _ => None,
  };
}

/// 標準アキ = 詰め可能量とする [`Aki`] を作る（約物アキは全量ベタまで詰められる）
fn aki(em: f32) -> Aki {
  return Aki {
    natural_em: em,
    shrink_em: em,
  };
}

#[cfg(test)]
mod tests {
  use super::{Aki, NIBU, Normalize, SHIBU, YakumonoClass, classify, gap, normalize};

  #[test]
  fn classify_sorts_each_class() {
    // Arrange / Act / Assert
    assert_eq!(classify('（'), YakumonoClass::Open);
    assert_eq!(classify('「'), YakumonoClass::Open);
    assert_eq!(classify('）'), YakumonoClass::Close);
    assert_eq!(classify('」'), YakumonoClass::Close);
    assert_eq!(classify('、'), YakumonoClass::Comma);
    assert_eq!(classify('。'), YakumonoClass::Comma);
    assert_eq!(classify('・'), YakumonoClass::MiddleDot);
    assert_eq!(classify('あ'), YakumonoClass::Normal);
    assert_eq!(classify('漢'), YakumonoClass::Normal);
  }

  #[test]
  fn gap_inserts_nibu_before_open_and_after_close_comma() {
    // Arrange / Act / Assert — 前アキ・後アキは二分
    assert_eq!(gap(YakumonoClass::Normal, YakumonoClass::Open), Some(aki(NIBU)), "kanji「");
    assert_eq!(gap(YakumonoClass::Close, YakumonoClass::Open), Some(aki(NIBU)), "」「");
    assert_eq!(gap(YakumonoClass::Comma, YakumonoClass::Open), Some(aki(NIBU)), "。「");
    assert_eq!(gap(YakumonoClass::Close, YakumonoClass::Normal), Some(aki(NIBU)), "」kanji");
    assert_eq!(gap(YakumonoClass::Comma, YakumonoClass::Normal), Some(aki(NIBU)), "。kanji");
  }

  #[test]
  fn gap_collapses_consecutive_and_inside_brackets() {
    // Arrange / Act / Assert — 連続約物・括弧内側はベタ（アキなし）
    assert_eq!(gap(YakumonoClass::Comma, YakumonoClass::Close), None, "。」");
    assert_eq!(gap(YakumonoClass::Close, YakumonoClass::Comma), None, "」。");
    assert_eq!(gap(YakumonoClass::Close, YakumonoClass::Close), None, "」」");
    assert_eq!(gap(YakumonoClass::Open, YakumonoClass::Normal), None, "「kanji");
    assert_eq!(gap(YakumonoClass::Normal, YakumonoClass::Close), None, "kanji」");
    assert_eq!(gap(YakumonoClass::Open, YakumonoClass::Open), None, "「「");
  }

  #[test]
  fn gap_uses_shibu_around_middle_dot() {
    // Arrange / Act / Assert — 中点は片側四分、括弧内側はベタ
    assert_eq!(gap(YakumonoClass::Normal, YakumonoClass::MiddleDot), Some(aki(SHIBU)), "kanji・");
    assert_eq!(gap(YakumonoClass::MiddleDot, YakumonoClass::Normal), Some(aki(SHIBU)), "・kanji");
    assert_eq!(gap(YakumonoClass::Open, YakumonoClass::MiddleDot), None, "「・");
    assert_eq!(gap(YakumonoClass::MiddleDot, YakumonoClass::Close), None, "・」");
  }

  #[test]
  fn gap_returns_none_for_normal_pair() {
    // Arrange / Act / Assert — 通常文字どうしは本表の対象外
    assert_eq!(gap(YakumonoClass::Normal, YakumonoClass::Normal), None);
  }

  #[test]
  fn normalize_trims_and_shifts_per_class() {
    // Arrange / Act / Assert
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
    assert_eq!(normalize(YakumonoClass::Normal), None, "通常文字は正規化しない");
  }

  /// テスト用: `super::aki` 相当（標準アキ = 詰め代）
  fn aki(em: f32) -> Aki {
    return Aki {
      natural_em: em,
      shrink_em: em,
    };
  }
}
