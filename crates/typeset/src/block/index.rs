//! 巻末索引ブロックの生成パス
//!
//! `seiran::build_pdf` が本文を一度ページ分割して各索引語の出現ページ（`model::PlacedIndexEntry`）を
//! 確定した**後**に走る、フォント依存の生成パスです（[`super::toc`] と同型）。`toc` と異なり右寄せ・
//! リーダーは使わず、1 エントリを「語 … ページ番号列（カンマ区切り、番号ごとに個別の内部リンク）」の
//! 単一行に組みます。
//!
//! `config` には依存せず、呼び出し側がプリミティブの [`IndexSpec`] / [`IndexEntryInput`] を組み立てて
//! 渡します。ソート（[`sort_index_entries`]）はこの module が ICU `Collator`（ロケール固定 `ja`）で行い、
//! `reading` があればそれを、なければ `word` をキーにします。

use font::{FontMetrics, shaper::HarfRustShapers};
use icu::{
  collator::{Collator, options::CollatorOptions},
  locale::locale,
};
use model::{AnchorId, Block, HBox, Length, Line, LineLink, LinkTarget, PositionedBox};

use super::Measurer;
use crate::lowering::TextStyle;

/// 索引生成に必要なプリミティブ設定（`config` 非依存）
#[derive(Debug, Clone)]
pub struct IndexSpec {
  /// 索引ページのタイトル文字列（例: `"Index"`）
  pub title: String,
  /// タイトル文字列の書体
  pub title_style: TextStyle,
  /// タイトルとエントリ群の間の縦アキ（pt）
  pub title_bottom_margin: Length,
  /// エントリの語部分の書体
  pub entry_style: TextStyle,
  /// ページ番号部分の書体（既存の参照リンク色を反映済み）
  pub page_number_style: TextStyle,
  /// 語とページ番号列の間の水平アキ（pt）
  pub entry_gap: Length,
  /// 行高係数。各行の行送り = 書体サイズ × この値
  pub line_height_factor: f32,
  /// 索引ブロック全体の下余白（pt）
  pub bottom_margin: Length,
}

/// 索引エントリが指す 1 出現ページ
#[derive(Debug, Clone)]
pub struct IndexPageRef {
  /// 表示するページ番号ラベル
  pub label: String,
  /// 出現ページの内部リンク到達先（本文内ページ index、0 起点）
  pub link_key: usize,
}

/// 1 索引エントリの入力
#[derive(Debug, Clone)]
pub struct IndexEntryInput {
  /// 索引語（表示テキスト）
  pub word: String,
  /// 読みソートキー（`[reading=...]`）。ソートにのみ使い、表示はしない
  pub reading: Option<String>,
  /// 出現ページ（昇順・重複なし）
  pub pages: Vec<IndexPageRef>,
}

/// エントリ列を ICU `Collator`（ロケール固定 `ja`）でソートする
///
/// キーは `reading` があればそれ、なければ `word`。安定ソートなので、キーが完全一致する
/// エントリ同士は入力の順序を保つ。
///
/// # Panics
///
/// `ja` ロケールの照合データはワークスペースの `icu`（`compiled_data`）に常に同梱されているため、
/// 実運用では発生しない。
pub fn sort_index_entries(entries: &mut [IndexEntryInput]) {
  let collator = Collator::try_new(locale!("ja").into(), CollatorOptions::default())
    .expect("ja ロケールの照合データは compiled_data で常に利用可能なはず");
  entries.sort_by(|a, b| {
    let key_a = a.reading.as_deref().unwrap_or(&a.word);
    let key_b = b.reading.as_deref().unwrap_or(&b.word);
    return collator.compare(key_a, key_b);
  });
}

/// 索引エントリ列を計測済みのブロック列に変換する
///
/// `entries` が空のときは空ベクタを返す（索引ページを出さない）。呼び出し前に
/// [`sort_index_entries`] でソート済みであることを期待する（本関数は並べ替えない）。
#[must_use]
pub fn build_index_blocks(
  spec: &IndexSpec,
  entries: &[IndexEntryInput],
  shapers: &HarfRustShapers,
  metrics: &FontMetrics,
) -> Vec<Block> {
  if entries.is_empty() {
    return Vec::new();
  }
  // 索引項目はハイフネーションしない（目次と同様・本文段落専用の #173 を踏襲）。
  // 和文約物アキ調整は本文と同じく既定で有効にする。
  let mut measurer = Measurer::new(shapers, metrics, Length::ZERO, 1.0, None, true);
  let mut blocks: Vec<Block> = Vec::new();

  blocks.push(Block::ComposedLine {
    line: compose_left_line(&mut measurer, &spec.title, spec.title_style),
    leading: spec.title_style.font_size * spec.line_height_factor,
  });
  if spec.title_bottom_margin.is_positive() {
    blocks.push(Block::fixed_space(spec.title_bottom_margin));
  }

  let entry_leading = spec.entry_style.font_size * spec.line_height_factor;
  for entry in entries {
    blocks.push(Block::ComposedLine {
      line: compose_entry_line(&mut measurer, spec, entry),
      leading: entry_leading,
    });
  }

  if spec.bottom_margin.is_positive() {
    blocks.push(Block::fixed_space(spec.bottom_margin));
  }
  return blocks;
}

/// 単一行を組み立てる際の累積状態（配置済みボックス・行の高さ）
#[derive(Default)]
struct LineAccum {
  /// 配置済みボックス列
  boxes: Vec<PositionedBox>,
  /// 行の高さ（ベースラインより上）
  height: Length,
  /// 行の深さ（ベースラインより下）
  depth: Length,
}

impl LineAccum {
  /// `HBox` 列を `x_start` から水平に並べて追加し、行の高さ・深さを更新する。末尾の x を返す
  fn place(&mut self, hboxes: Vec<HBox>, x_start: Length) -> Length {
    let mut x = x_start;
    for hbox in hboxes {
      self.height = self.height.max(hbox.height);
      self.depth = self.depth.max(hbox.depth);
      self.boxes.push(PositionedBox {
        content: hbox.content,
        x,
        dy: Length::ZERO,
        width: hbox.width,
      });
      x += hbox.width;
    }
    return x;
  }

  /// 累積した内容を `Line`（段落最終行扱い）に確定する
  fn into_line(self, links: Vec<LineLink>) -> Line {
    return Line {
      boxes: self.boxes,
      height: self.height,
      depth: self.depth,
      is_last: true,
      links,
      footnotes: Vec::new(),
      index_marks: Vec::new(),
    };
  }
}

/// テキストを左端（x=0）からシェーピングして単一行に組む（タイトル行用）
fn compose_left_line(measurer: &mut Measurer, text: &str, style: TextStyle) -> Line {
  let mut acc = LineAccum::default();
  acc.place(measurer.shape_text(text, style), Length::ZERO);
  return acc.into_line(Vec::new());
}

/// 1 エントリを「語 … ページ番号列（カンマ区切り）」の単一行に組む
///
/// ページ番号ごとに個別の [`LineLink`] を張るので、複数ページにまたがるエントリでも
/// クリックした番号に対応するページへ飛べる。
fn compose_entry_line(measurer: &mut Measurer, spec: &IndexSpec, entry: &IndexEntryInput) -> Line {
  let mut acc = LineAccum::default();
  let mut links = Vec::new();

  let mut x = acc.place(measurer.shape_text(&entry.word, spec.entry_style), Length::ZERO);
  if !entry.pages.is_empty() {
    x += spec.entry_gap;
  }

  for (i, page) in entry.pages.iter().enumerate() {
    if i > 0 {
      x = acc.place(measurer.shape_text(", ", spec.entry_style), x);
    }
    let start_x = x;
    x = acc.place(measurer.shape_text(&page.label, spec.page_number_style), x);
    links.push(LineLink {
      target: LinkTarget::Internal(AnchorId::IndexPage(page.link_key)),
      x0: start_x,
      x1: x,
    });
  }

  return acc.into_line(links);
}

#[cfg(test)]
mod tests {
  use model::{AnchorId, LinkTarget};

  use super::{IndexEntryInput, IndexPageRef, sort_index_entries};

  fn entry(word: &str, reading: Option<&str>) -> IndexEntryInput {
    return IndexEntryInput {
      word: word.to_string(),
      reading: reading.map(str::to_string),
      pages: vec![IndexPageRef {
        label: "1".to_string(),
        link_key: 0,
      }],
    };
  }

  #[test]
  fn sort_index_entries_prefers_reading_over_word() {
    // Arrange — 語の字面では逆順だが、reading を使えば五十音順になる語を用意する
    let mut entries = vec![entry("後", Some("うしろ")), entry("前", Some("あいうえお"))];

    // Act
    sort_index_entries(&mut entries);

    // Assert — reading（あいうえお < うしろ）でソートされる
    assert_eq!(entries[0].word, "前");
    assert_eq!(entries[1].word, "後");
  }

  #[test]
  fn sort_index_entries_falls_back_to_word_without_reading() {
    // Arrange
    let mut entries = vec![entry("b", None), entry("a", None)];

    // Act
    sort_index_entries(&mut entries);

    // Assert
    assert_eq!(entries[0].word, "a");
    assert_eq!(entries[1].word, "b");
  }

  #[test]
  fn sort_index_entries_is_stable_for_equal_keys() {
    // Arrange — 同じ語が異なる情報（ここではダミーの pages）で 2 件存在する場合、入力順を保つ
    let mut entries = vec![
      IndexEntryInput {
        word: "same".to_string(),
        reading: None,
        pages: vec![IndexPageRef {
          label: "1".to_string(),
          link_key: 0,
        }],
      },
      IndexEntryInput {
        word: "same".to_string(),
        reading: None,
        pages: vec![IndexPageRef {
          label: "2".to_string(),
          link_key: 1,
        }],
      },
    ];

    // Act
    sort_index_entries(&mut entries);

    // Assert — 安定ソートなので最初のページが 1 のままのはず
    assert_eq!(entries[0].pages[0].label, "1");
    assert_eq!(entries[1].pages[0].label, "2");
  }

  // フォントを使う build_index_blocks の配置検証（リンク span・列幅）はフォント依存のため
  // `seiran` の結合テストで確認する。ここでは純粋ロジック（ソート）のみ検証する。
  #[test]
  fn link_target_wraps_link_key() {
    let key = 3;
    assert!(
      matches!(LinkTarget::Internal(AnchorId::IndexPage(key)), LinkTarget::Internal(AnchorId::IndexPage(k)) if k == key)
    );
  }
}
