//! タイトルページ（`\maketitle` 相当）の lowering
//!
//! ソース側にコマンドを追加せず、`config.toml` の `[document]` メタデータ（title / author /
//! date）と `style.toml` の `[title_page]` から自動生成する。タイトル・著者・日付を中央寄せの
//! 段落として縦に積んだ [`LayoutNode::VBox`]（`align = Center`）に、本文を次ページから始める
//! ための [`LayoutNode::PageBreak`] を続けた列を返す。
//!
//! 空（`None` または空白のみ）の要素は描画しない。要素間の縦アキは「直前要素の下マージン」を
//! 次の要素の前に挿入することで、欠落要素があってもアキが二重に出ないようにする。

use config::read_style::TitlePageStyle;
use model::{Align, FontKind, Length};

use super::layout_node::{LayoutNode, TextStyle};

/// タイトルページに載せる文書メタデータ。
///
/// `config.toml` の `[document]` から `build_pdf` が組み立てる。各要素は `Option` で、
/// 未設定（`None`）または空白のみの場合はタイトルページに描画しない。
#[derive(Debug, Clone, Default)]
pub struct TitlePageMetadata {
  /// タイトル（`[document] title`）
  pub title: Option<String>,
  /// 著者（`[document] author`）
  pub author: Option<String>,
  /// 日付（`[document] date`）
  pub date: Option<String>,
}

/// タイトルページのレイアウトノード列を生成する。
///
/// 戻り値は `[VBox(中央寄せ), PageBreak]`（メタデータが全て空なら `[PageBreak]`）。`build_pdf` が
/// この列を本文の先頭に prepend することで、タイトルページが先頭ページ・本文が次ページから始まる。
///
/// 呼び出し側は `style.title_page.enabled` が真のときだけ呼ぶこと（有効判定はここでは行わない）。
#[must_use]
pub fn lower_title_page(meta: &TitlePageMetadata, style: &TitlePageStyle) -> Vec<LayoutNode> {
  // タイトル/著者/日付の本体（テキスト + 要素間アキ）を先に組み立てる。
  let mut body: Vec<LayoutNode> = Vec::new();

  // (テキスト, フォントサイズ, フォント種別, この要素の下マージン) を文書順に並べる。
  let entries: [(Option<&str>, Length, FontKind, Length); 3] = [
    (meta.title.as_deref(), style.title_font_size, style.title_font_kind, style.title_bottom_margin),
    (meta.author.as_deref(), style.author_font_size, style.author_font_kind, style.author_bottom_margin),
    (meta.date.as_deref(), style.date_font_size, style.date_font_kind, Length::pt(0.0)),
  ];

  // 直前に積んだ要素の下マージン。次の present な要素を積む直前に Vkern として挿入する。
  let mut pending_gap: Option<Length> = None;
  for (text, font_size, font_kind, gap_after) in entries {
    let Some(text) = text.map(str::trim).filter(|trimmed| !trimmed.is_empty()) else {
      continue;
    };
    if let Some(gap) = pending_gap.take()
      && gap.is_positive()
    {
      body.push(LayoutNode::Vkern { length: gap });
    }
    body.push(LayoutNode::Text(
      text.to_string(),
      TextStyle {
        font_size,
        font_kind,
        color: None,
      },
    ));
    pending_gap = Some(gap_after);
  }

  let mut result: Vec<LayoutNode> = Vec::with_capacity(2);
  // 載せる要素が 1 つでもあるときだけ中央寄せ VBox を出す（空のタイトルページは作らない）。
  if !body.is_empty() {
    let mut children: Vec<LayoutNode> = Vec::with_capacity(body.len() + 1);
    // ページ上端からタイトルまでの送り（垂直位置の簡易指定）。本体があるときだけ付ける。
    if style.top_margin.is_positive() {
      children.push(LayoutNode::Vkern {
        length: style.top_margin,
      });
    }
    children.extend(body);
    result.push(LayoutNode::VBox {
      children,
      margin_bottom: Length::pt(0.0),
      indent: Length::pt(0.0),
      right_indent: Length::pt(0.0),
      align: Align::Center,
    });
  }
  // タイトルページの後に改ページし、本文を次ページから始める
  result.push(LayoutNode::PageBreak);
  return result;
}

#[cfg(test)]
mod tests {
  use config::read_style::TitlePageStyle;
  use model::{Align, Length};

  use super::{super::layout_node::LayoutNode, TitlePageMetadata, lower_title_page};

  /// 中央寄せ `VBox` の子ノードを取り出すヘルパ
  fn title_vbox_children(nodes: &[LayoutNode]) -> &[LayoutNode] {
    for node in nodes {
      if let LayoutNode::VBox {
        children, align, ..
      } = node
      {
        assert_eq!(*align, Align::Center, "タイトルページの VBox は中央寄せ");
        return children;
      }
    }
    panic!("VBox が見つからない: {nodes:?}");
  }

  /// 子ノードから Text 文字列だけを文書順に集めるヘルパ
  fn texts(children: &[LayoutNode]) -> Vec<String> {
    return children
      .iter()
      .filter_map(|n| match n {
        LayoutNode::Text(text, _) => Some(text.clone()),
        _ => None,
      })
      .collect();
  }

  #[test]
  fn full_metadata_yields_centered_vbox_then_page_break() {
    // Arrange
    let meta = TitlePageMetadata {
      title: Some("My Title".to_string()),
      author: Some("Me".to_string()),
      date: Some("2026-06-15".to_string()),
    };
    let style = TitlePageStyle::default();

    // Act
    let nodes = lower_title_page(&meta, &style);

    // Assert — 末尾は PageBreak、VBox は中央寄せでタイトル/著者/日付の順
    assert!(matches!(nodes.last(), Some(LayoutNode::PageBreak)), "末尾は PageBreak: {nodes:?}");
    let children = title_vbox_children(&nodes);
    assert_eq!(texts(children), vec!["My Title", "Me", "2026-06-15"]);
  }

  #[test]
  fn title_uses_style_font_size_and_kind() {
    // Arrange — タイトルのフォントサイズ・種別がスタイルから反映される
    let meta = TitlePageMetadata {
      title: Some("T".to_string()),
      ..TitlePageMetadata::default()
    };
    let style = TitlePageStyle {
      title_font_size: Length::pt(40.0),
      ..TitlePageStyle::default()
    };

    // Act
    let nodes = lower_title_page(&meta, &style);

    // Assert
    let children = title_vbox_children(&nodes);
    let text_style = children
      .iter()
      .find_map(|n| match n {
        LayoutNode::Text(t, s) if t == "T" => Some(*s),
        _ => None,
      })
      .expect("Text が見つからない");
    assert_eq!(text_style.font_size, Length::pt(40.0));
    assert_eq!(text_style.font_kind, style.title_font_kind);
  }

  #[test]
  fn missing_author_skips_element_and_its_gap() {
    // Arrange — 著者なし。タイトルと日付だけが並び、間の Vkern は 1 つ（title_bottom_margin）
    let meta = TitlePageMetadata {
      title: Some("T".to_string()),
      author: None,
      date: Some("D".to_string()),
    };
    let style = TitlePageStyle::default();

    // Act
    let nodes = lower_title_page(&meta, &style);

    // Assert — Text は T, D の 2 つ。両者の間に Vkern が 1 つだけ
    let children = title_vbox_children(&nodes);
    assert_eq!(texts(children), vec!["T", "D"]);
    let vkern_count = children.iter().filter(|n| matches!(n, LayoutNode::Vkern { .. })).count();
    // 先頭 top_margin Vkern + 要素間 1 つ = 2
    assert_eq!(vkern_count, 2, "top_margin と要素間アキで Vkern は 2 つ: {children:?}");
  }

  #[test]
  fn empty_metadata_yields_only_page_break() {
    // Arrange — メタデータが全て空のとき VBox は出さず PageBreak のみ
    let meta = TitlePageMetadata::default();
    let style = TitlePageStyle::default();

    // Act
    let nodes = lower_title_page(&meta, &style);

    // Assert
    assert_eq!(nodes.len(), 1);
    assert!(matches!(nodes[0], LayoutNode::PageBreak));
  }

  #[test]
  fn blank_title_is_treated_as_empty() {
    // Arrange — 空白のみのタイトルは描画しない
    let meta = TitlePageMetadata {
      title: Some("   ".to_string()),
      author: Some("A".to_string()),
      ..TitlePageMetadata::default()
    };
    let style = TitlePageStyle::default();

    // Act
    let nodes = lower_title_page(&meta, &style);

    // Assert — 著者だけが残る
    let children = title_vbox_children(&nodes);
    assert_eq!(texts(children), vec!["A"]);
  }
}
